//! Translation of displayed text.
//!
//! [`tr`] takes a phrase as it is written in the source and returns it in
//! the language the current request asked for, falling back to the source
//! text when there is no translation. It is deliberately **synchronous and
//! database-free**: the catalog is small enough to hold in memory, so
//! translation can happen anywhere, including inside a Sailfish template,
//! which cannot await.
//!
//! Phrases stock themselves. The first time a phrase is translated, it is
//! queued for insertion so that it shows up in the admin as outstanding
//! work; a translator can also enter phrases there ahead of the text
//! appearing. Discovery happens off the request path -- a page render never
//! waits on it and never fails because of it.
//!
//! The unit is a whole phrase, sentence or paragraph rather than a word, so
//! two identical strings almost always want the same translation. Where they
//! don't, [`tr_ctx`] distinguishes them.

use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, OnceLock, RwLock},
};

use anyhow::Error;
use common::{shared::db, timestring::TimeString};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::{Sender, channel};

use crate::peering::LoadStorePeered;

/// A language the site can be displayed in.
///
/// `En` is the language phrases are written in, so it is never a
/// translation: it is what everything falls back to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lang {
    #[default]
    En,
    Es,
    Pt,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::En, Lang::Es, Lang::Pt];

    /// The languages a phrase can be translated *into*, which is all of them
    /// except the one phrases are written in.
    pub const TRANSLATABLE: [Lang; 2] = [Lang::Es, Lang::Pt];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Es => "es",
            Lang::Pt => "pt",
        }
    }

    /// The language's name in itself, for a language picker: someone who
    /// cannot read the current language still has to find their own.
    pub fn endonym(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Es => "Español",
            Lang::Pt => "Português",
        }
    }

    /// The language named by a BCP 47 tag, matched on the primary subtag so
    /// that `pt-BR` and `es-419` land where they should.
    pub fn from_tag(tag: &str) -> Option<Lang> {
        let primary = tag.split('-').next()?.trim().to_ascii_lowercase();

        Lang::ALL.into_iter().find(|lang| lang.code() == primary)
    }

    /// The best supported language named in an `Accept-Language` header.
    ///
    /// Entries for languages the site does not have are skipped rather than
    /// counted, so `fr;q=1.0, pt-BR;q=0.8` still yields Portuguese.
    pub fn from_accept_language(header: &str) -> Option<Lang> {
        let mut best: Option<(f32, Lang)> = None;

        for entry in header.split(',') {
            let mut parts = entry.split(';');

            let Some(tag) = parts.next().map(str::trim) else {
                continue;
            };

            let mut weight = 1.0f32;

            for param in parts {
                if let Some(value) = param.trim().strip_prefix("q=") {
                    weight = value.parse().unwrap_or(0.0);
                }
            }

            let Some(lang) = Lang::from_tag(tag) else {
                continue;
            };

            if weight > 0.0 && best.is_none_or(|(best_weight, _)| weight > best_weight) {
                best = Some((weight, lang));
            }
        }

        best.map(|(_, lang)| lang)
    }

    fn index(self) -> usize {
        match self {
            Lang::En => 0,
            Lang::Es => 1,
            Lang::Pt => 2,
        }
    }
}

tokio::task_local! {
    /// The language of the request being served.
    ///
    /// Set once by [`crate::middleware::language::LanguageMiddleware`] so
    /// that [`tr`] can be called from a deeply nested template without every
    /// component in the chain having to carry a language along. Unset --
    /// in a background task, or a test -- means English, which is the same
    /// as having no translation, so nothing breaks when it is missing.
    static CURRENT_LANG: Lang;
}

/// The language of the request being served, or English outside a request.
pub fn current_lang() -> Lang {
    CURRENT_LANG.try_with(|lang| *lang).unwrap_or_default()
}

/// Run `future` with `lang` as the current language.
pub async fn with_lang<F: Future>(lang: Lang, future: F) -> F::Output {
    CURRENT_LANG.scope(lang, future).await
}

/// Translate a phrase into the language of the current request.
///
/// Takes a `&'static str` on purpose. A phrase has to be a literal in the
/// source, not a value assembled at runtime: interpolating a name or a count
/// into the text before translating it would stock the phrase table with a
/// row per distinct value and bury the real phrases. Put a placeholder in
/// the phrase and substitute after translating:
///
/// ```ignore
/// tr("Welcome back, {name}").replace("{name}", &user.name)
/// ```
pub fn tr(source: &'static str) -> String {
    translate("", source, current_lang(), Origin::Discovered)
}

/// Translate a phrase whose English wording means something different here
/// than it does elsewhere.
///
/// Rarely needed -- phrases are whole sentences or paragraphs, which seldom
/// collide -- but a short label like "Open" can be a verb in one place and
/// an adjective in another, and those want different translations in most
/// languages. `context` names the sense, not the location: phrases sharing a
/// sense should share a context so they are translated once.
pub fn tr_ctx(context: &'static str, source: &'static str) -> String {
    translate(context, source, current_lang(), Origin::Discovered)
}

/// Translate into a language chosen explicitly rather than by the request.
///
/// For the few callers that render text in a language other than the one
/// they are being read in -- an email to a subscriber, say.
pub fn tr_in(lang: Lang, source: &'static str) -> String {
    translate("", source, lang, Origin::Discovered)
}

/// Translate a phrase for use inside a `<script>` block.
///
/// Returns a complete JavaScript string literal, quotes included, and is
/// rendered raw with `<%- %>`. HTML escaping is wrong inside a script
/// element -- a browser does not decode entities there, so an escaped
/// apostrophe would reach the reader as `&#39;` -- and a translation is
/// exactly the kind of text that acquires one.
pub fn tr_js(source: &'static str) -> String {
    serde_json::to_string(&tr(source)).unwrap_or_else(|_| String::from("\"\""))
}

/// The longest phrase that will be stocked from page content. Above this the
/// text is still displayed and can still be translated if someone enters it
/// by hand; it just isn't added automatically.
pub const MAX_CONTENT_PHRASE: usize = 8192;

/// Translate text that came from the database rather than from the source.
///
/// Authored copy -- what a page-builder template's `{$ tr $}` directive
/// carries -- is data, and so cannot be a `&'static str`. That makes this the
/// one entry point where a caller could hand the translator something
/// unbounded, so it stocks only what looks like authored copy and marks the
/// rows as content, keeping them distinguishable from -- and purgeable
/// without disturbing -- the phrases that come from the source.
///
/// `context` does what it does for [`tr_ctx`], and is empty for almost
/// everything. It is a parameter rather than a second function because the
/// directive always has one to pass, empty or not.
pub fn tr_content(context: &str, source: &str) -> String {
    // Oversized text is displayed, and can still be translated by someone
    // entering it by hand; it just doesn't stock itself.
    if source.len() <= MAX_CONTENT_PHRASE {
        discover(context, source, Origin::Content);
    }

    translated(context, source, current_lang())
}

/// Whether a phrase has been translated into `lang`, and what it says.
///
/// [`tr`] and its relatives answer "what should this reader see", which is
/// the source text whenever nothing has been translated -- the fallback that
/// keeps every page readable. A caller that has to tell those two apart asks
/// this instead: a page offering to be read in another language can only
/// honestly offer the languages it has actually been written in, and
/// something assembled out of several phrases -- a badge, say -- may want to
/// be shown in a translation only once all of them have one.
///
/// Discovery happens here as it does in [`tr`], so asking whether a phrase
/// has been translated is also how it turns up in the admin to be
/// translated.
pub fn translation(context: &'static str, source: &'static str, lang: Lang) -> Option<String> {
    discover(context, source, Origin::Discovered);

    lookup(context, source, lang)
}

fn translate(context: &str, source: &str, lang: Lang, origin: Origin) -> String {
    // Discovery happens whatever the language, including English. Most
    // traffic is English, and a phrase that only stocked itself when someone
    // happened to read the page in Spanish would leave translators with an
    // empty list until after the fact.
    discover(context, source, origin);

    translated(context, source, lang)
}

/// What the running site's catalog says, or `None` to display it as written.
fn lookup(context: &str, source: &str, lang: Lang) -> Option<String> {
    CATALOG
        .read()
        .expect("translation catalog lock poisoned")
        .lookup(context, source, lang)
        .map(str::to_owned)
}

/// What the running site's catalog says, falling back to the source text.
fn translated(context: &str, source: &str, lang: Lang) -> String {
    lookup(context, source, lang).unwrap_or_else(|| source.to_owned())
}

/// Where a phrase came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// A literal in the source, found by [`tr`].
    #[default]
    Discovered,
    /// Authored text from the database, found by [`tr_content`].
    Content,
    /// Entered in the admin before the text appeared anywhere.
    Manual,
}

impl Origin {
    /// How the origin is written in the database. The `serde` renaming above
    /// spells it the same way, so a phrase reads the same whether it arrives
    /// from a table or from a peer.
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Discovered => "discovered",
            Origin::Content => "content",
            Origin::Manual => "manual",
        }
    }

    /// The origin a stored value names. Anything unrecognized reads as
    /// `Discovered`: the origin is a note about where a phrase was first
    /// seen, and the phrase is worth keeping even when the note isn't.
    pub fn from_str(value: &str) -> Origin {
        match value {
            "content" => Origin::Content,
            "manual" => Origin::Manual,
            _ => Origin::Discovered,
        }
    }
}

/// The identity of a phrase, derived rather than generated so that every
/// instance agrees on it without having to coordinate.
pub fn phrase_id(context: &str, source: &str) -> String {
    let mut hasher = Sha256::new();

    hasher.update(context.as_bytes());
    // Separator, so ("ab", "c") and ("a", "bc") are different phrases.
    hasher.update([0x1f]);
    hasher.update(source.as_bytes());

    format!("{:x}", hasher.finalize())
}

type Translations = [Option<String>; Lang::ALL.len()];

/// Every phrase the site knows how to say in another language.
///
/// A type rather than a bare map so that the two rules about what counts as
/// a translation -- English is never one, and neither is a blank -- are
/// written once, and so that they can be exercised on a catalog of the
/// test's own rather than on the one the whole process shares.
#[derive(Debug, Default)]
struct Catalog {
    /// Nested by context and then by source text, rather than keyed by a
    /// tuple, so a lookup can be done with two `&str` and no allocation on a
    /// code path that runs many times per page.
    phrases: HashMap<String, HashMap<String, Translations>>,
}

impl Catalog {
    /// What `source` says in `lang`, or `None` to display it as written.
    fn lookup(&self, context: &str, source: &str, lang: Lang) -> Option<&str> {
        // English is what phrases are written in, so there is nothing to
        // look up: the source text is already the answer.
        if lang == Lang::En {
            return None;
        }

        self.phrases
            .get(context)
            .and_then(|phrases| phrases.get(source))
            .and_then(|translations| translations[lang.index()].as_deref())
            // A blank is a language somebody has not got to yet.
            .filter(|translated| !translated.is_empty())
    }

    /// Record everything `phrase` says, replacing what was known about it.
    fn apply(&mut self, phrase: &Phrase) {
        let translations = self
            .phrases
            .entry(phrase.context.clone())
            .or_default()
            .entry(phrase.source.clone())
            .or_default();

        for lang in Lang::ALL {
            let translated = phrase.translation(lang);

            translations[lang.index()] = if translated.is_empty() {
                None
            } else {
                Some(translated.to_owned())
            };
        }
    }
}

/// The catalog the running site translates against.
static CATALOG: LazyLock<RwLock<Catalog>> = LazyLock::new(|| RwLock::new(Catalog::default()));

/// How many distinct phrases one process will discover before it stops
/// looking. A generous ceiling on a site whose phrase count should be in the
/// low thousands -- it exists so that a caller passing user input to
/// [`tr_content`] degrades into "translation stops noticing new phrases"
/// rather than "the phrase table grows without limit".
const DISCOVERY_LIMIT: usize = 50_000;

/// Phrases this process has already queued, so that a phrase on a busy page
/// is written once rather than once per render.
static DISCOVERED: LazyLock<RwLock<HashSet<(String, String)>>> =
    LazyLock::new(|| RwLock::new(HashSet::new()));

static DISCOVERY_QUEUE: OnceLock<Sender<Discovery>> = OnceLock::new();

#[derive(Debug)]
struct Discovery {
    context: String,
    source: String,
    origin: Origin,
}

fn discover(context: &str, source: &str, origin: Origin) {
    if source.trim().is_empty() {
        return;
    }

    let Some(queue) = DISCOVERY_QUEUE.get() else {
        // Before startup finished, or in a test. Nothing to record into.
        return;
    };

    let key = (context.to_owned(), source.to_owned());

    {
        let discovered = DISCOVERED.read().expect("discovery set lock poisoned");

        if discovered.contains(&key) {
            return;
        }

        if discovered.len() >= DISCOVERY_LIMIT {
            return;
        }
    }

    {
        let mut discovered = DISCOVERED.write().expect("discovery set lock poisoned");

        if !discovered.insert(key.clone()) {
            return;
        }

        if discovered.len() == DISCOVERY_LIMIT {
            tracing::warn!(
                limit = DISCOVERY_LIMIT,
                "translation discovery limit reached; no further phrases will be recorded \
                 by this process. Something is probably translating values rather than phrases."
            );
        }
    }

    // Never blocks: a full queue means the writer is behind, and a phrase is
    // not worth making a page wait for. Dropping one costs nothing, because
    // forgetting it here means the next render of the same phrase queues it
    // again.
    if queue
        .try_send(Discovery {
            context: context.to_owned(),
            source: source.to_owned(),
            origin,
        })
        .is_err()
    {
        DISCOVERED
            .write()
            .expect("discovery set lock poisoned")
            .remove(&key);
    }
}

/// Load the catalog and start the background writer.
///
/// Called once during startup, before the server begins accepting requests,
/// so that the first page rendered is already translated.
pub async fn start() -> Result<(), Error> {
    reload_catalog().await?;

    let (sender, mut receiver) = channel::<Discovery>(4096);

    if DISCOVERY_QUEUE.set(sender).is_err() {
        return Ok(());
    }

    tokio::spawn(async move {
        while let Some(discovery) = receiver.recv().await {
            if let Err(err) = record(&discovery).await {
                tracing::warn!("unable to record discovered phrase: {err}");
            }
        }
    });

    Ok(())
}

/// Write a newly seen phrase, and a blank translation for every language it
/// could be translated into, so the admin shows a complete grid.
///
/// Note what this is *not*: `INSERT OR REPLACE`, the house style elsewhere.
/// This runs whenever a phrase is displayed, so replacing the row would let
/// an ordinary page render silently discard a translation somebody had
/// written days earlier.
async fn record(discovery: &Discovery) -> Result<(), Error> {
    record_into(
        db(),
        &discovery.context,
        &discovery.source,
        discovery.origin,
    )
    .await
}

/// The statements [`record`] runs, over a pool the caller names, so that
/// what they do to an existing row can be tested.
async fn record_into(
    pool: &sqlx::SqlitePool,
    context: &str,
    source: &str,
    origin: Origin,
) -> Result<(), Error> {
    let id = phrase_id(context, source);
    let now = TimeString::now().to_string();
    let origin = origin.as_str();

    sqlx::query!(
        r#"INSERT INTO translation_phrase
             (phrase_id, context, source, origin, first_seen, last_seen)
           VALUES (?, ?, ?, ?, ?, ?)
           ON CONFLICT (phrase_id) DO UPDATE SET last_seen = excluded.last_seen"#,
        id,
        context,
        source,
        origin,
        now,
        now,
    )
    .execute(pool)
    .await?;

    for lang in Lang::TRANSLATABLE {
        let code = lang.code();

        sqlx::query!(
            r#"INSERT INTO translation (phrase_id, lang, translated, updated)
               VALUES (?, ?, '', ?)
               ON CONFLICT (phrase_id, lang) DO NOTHING"#,
            id,
            code,
            now,
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// Read every translation out of the database and into memory.
pub async fn reload_catalog() -> Result<(), Error> {
    let rows = sqlx::query!(
        r#"SELECT p.context, p.source, t.lang, t.translated
           FROM translation_phrase p
           JOIN translation t ON t.phrase_id = p.phrase_id
           WHERE t.translated != ''"#
    )
    .fetch_all(db())
    .await?;

    let mut catalog = Catalog::default();

    for row in rows {
        let Some(lang) = Lang::from_tag(&row.lang) else {
            continue;
        };

        catalog
            .phrases
            .entry(row.context)
            .or_default()
            .entry(row.source)
            .or_default()[lang.index()] = Some(row.translated);
    }

    *CATALOG.write().expect("translation catalog lock poisoned") = catalog;

    Ok(())
}

/// A phrase and everything known about how to say it.
///
/// The unit of both editing and peering: saving one from the admin writes it
/// here and hands it to every other frontend instance, each of which stores
/// it and refreshes its own catalog.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Phrase {
    pub phrase_id: String,
    pub context: String,
    pub source: String,
    pub origin: Origin,
    /// Keyed by language code. A language missing here, or present with an
    /// empty string, displays as the source text.
    pub translations: HashMap<String, String>,
}

impl Phrase {
    pub fn new(context: &str, source: &str, origin: Origin) -> Phrase {
        Phrase {
            phrase_id: phrase_id(context, source),
            context: context.to_owned(),
            source: source.to_owned(),
            origin,
            translations: HashMap::new(),
        }
    }

    pub fn translation(&self, lang: Lang) -> &str {
        self.translations
            .get(lang.code())
            .map(String::as_str)
            .unwrap_or_default()
    }

    /// The statements a save runs, over a pool the caller names.
    async fn write_into(&self, pool: &sqlx::SqlitePool) -> Result<(), Error> {
        let now = TimeString::now().to_string();
        let origin = self.origin.as_str();

        // Upsert rather than INSERT OR REPLACE so that a phrase keeps the
        // date it was first seen.
        sqlx::query!(
            r#"INSERT INTO translation_phrase
                 (phrase_id, context, source, origin, first_seen, last_seen)
               VALUES (?, ?, ?, ?, ?, ?)
               ON CONFLICT (phrase_id) DO UPDATE SET
                 context = excluded.context,
                 source = excluded.source,
                 origin = excluded.origin"#,
            self.phrase_id,
            self.context,
            self.source,
            origin,
            now,
            now,
        )
        .execute(pool)
        .await?;

        for lang in Lang::TRANSLATABLE {
            let code = lang.code();
            let translated = self.translation(lang);

            sqlx::query!(
                r#"INSERT INTO translation (phrase_id, lang, translated, updated)
                   VALUES (?, ?, ?, ?)
                   ON CONFLICT (phrase_id, lang) DO UPDATE SET
                     translated = excluded.translated,
                     updated = excluded.updated"#,
                self.phrase_id,
                code,
                translated,
                now,
            )
            .execute(pool)
            .await?;
        }

        Ok(())
    }

    /// Update the in-memory catalog for this one phrase, so an edit takes
    /// effect without re-reading the whole table.
    fn apply_to_catalog(&self) {
        CATALOG
            .write()
            .expect("translation catalog lock poisoned")
            .apply(self);
    }
}

impl LoadStorePeered for Phrase {
    type DatabaseKey = String;

    const GROUP: &str = "translation_phrase";

    #[tracing::instrument]
    async fn read_from_database(id: Self::DatabaseKey) -> Result<Self, Error> {
        let phrase = sqlx::query!(
            r#"SELECT phrase_id, context, source, origin
               FROM translation_phrase WHERE phrase_id = ?"#,
            id
        )
        .fetch_one(db())
        .await?;

        let rows = sqlx::query!(
            r#"SELECT lang, translated FROM translation WHERE phrase_id = ?"#,
            id
        )
        .fetch_all(db())
        .await?;

        Ok(Phrase {
            phrase_id: phrase.phrase_id,
            context: phrase.context,
            source: phrase.source,
            origin: Origin::from_str(&phrase.origin),
            translations: rows
                .into_iter()
                .map(|row| (row.lang, row.translated))
                .collect(),
        })
    }

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        self.write_into(db()).await?;

        // Both the local save and the one a peer performs on receiving the
        // broadcast come through here, so this is where an edit reaches
        // every instance's catalog.
        self.apply_to_catalog();

        Ok(())
    }

    #[tracing::instrument]
    async fn delete_from_database(id: Self::DatabaseKey) -> Result<(), Error> {
        // The phrase's translations go with it, by foreign key.
        sqlx::query!(r#"DELETE FROM translation_phrase WHERE phrase_id = ?"#, id)
            .execute(db())
            .await?;

        reload_catalog().await?;

        Ok(())
    }
}

/// Put `phrase` into the catalog the running site translates against.
///
/// That catalog is process-wide, so this is only for a test that has to show
/// translation reaching some caller -- a template directive, say. A test
/// about the lookup rules themselves wants a [`Catalog`] of its own instead,
/// and a test using this wants a phrase no other test would write.
#[cfg(test)]
pub fn install_for_test(phrase: &Phrase) {
    phrase.apply_to_catalog();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regional_tags_match_their_language() {
        assert_eq!(Lang::from_tag("pt-BR"), Some(Lang::Pt));
        assert_eq!(Lang::from_tag("es-419"), Some(Lang::Es));
        assert_eq!(Lang::from_tag("fr"), None);
    }

    #[test]
    fn the_heaviest_supported_language_wins() {
        assert_eq!(
            Lang::from_accept_language("fr;q=1.0, es;q=0.4, pt-BR;q=0.9"),
            Some(Lang::Pt)
        );
        assert_eq!(Lang::from_accept_language("es, pt;q=0.5"), Some(Lang::Es));
    }

    #[test]
    fn a_refused_language_is_not_chosen() {
        assert_eq!(Lang::from_accept_language("es;q=0"), None);
    }

    #[test]
    fn nothing_recognizable_is_no_language() {
        assert_eq!(Lang::from_accept_language("*"), None);
        assert_eq!(Lang::from_accept_language("fr, de;q=0.8"), None);
    }

    #[test]
    fn context_is_part_of_a_phrase_s_identity() {
        assert_ne!(phrase_id("", "Open"), phrase_id("verb", "Open"));

        // The separator keeps a context/source split from being ambiguous.
        assert_ne!(phrase_id("ab", "c"), phrase_id("a", "bc"));
    }

    #[test]
    fn identity_does_not_depend_on_which_instance_asks() {
        assert_eq!(phrase_id("", "Sign in"), phrase_id("", "Sign in"));
    }

    /// A phrase translated into Spanish and pointedly not into Portuguese.
    fn signed_in() -> Phrase {
        let mut phrase = Phrase::new("", "Sign in", Origin::Manual);

        phrase
            .translations
            .insert(String::from("es"), String::from("Iniciar sesión"));
        phrase
            .translations
            .insert(String::from("pt"), String::new());

        phrase
    }

    #[test]
    fn an_untranslated_phrase_has_nothing_to_look_up() {
        let catalog = Catalog::default();

        assert_eq!(catalog.lookup("", "Sign in", Lang::Es), None);
    }

    #[test]
    fn english_is_never_a_translation() {
        let mut catalog = Catalog::default();
        catalog.apply(&signed_in());

        assert_eq!(catalog.lookup("", "Sign in", Lang::En), None);
    }

    #[test]
    fn a_translation_is_used_when_there_is_one() {
        let mut catalog = Catalog::default();
        catalog.apply(&signed_in());

        assert_eq!(
            catalog.lookup("", "Sign in", Lang::Es),
            Some("Iniciar sesión")
        );
    }

    /// A language left blank is work outstanding, not a translation into the
    /// empty string, so the reader gets the English.
    #[test]
    fn a_blank_translation_is_not_one() {
        let mut catalog = Catalog::default();
        catalog.apply(&signed_in());

        assert_eq!(catalog.lookup("", "Sign in", Lang::Pt), None);
    }

    /// The same words under a different context are a different phrase, and
    /// a lookup must not stray from one to the other.
    #[test]
    fn a_context_is_looked_up_separately() {
        let mut catalog = Catalog::default();
        catalog.apply(&signed_in());

        assert_eq!(catalog.lookup("button", "Sign in", Lang::Es), None);
    }

    /// The fallback the whole scheme rests on: nothing in the catalog means
    /// the page still says something, in the language it was written in.
    #[test]
    fn an_untranslated_phrase_reads_as_its_source() {
        assert_eq!(
            translated("", "no phrase says this", Lang::Es),
            "no phrase says this"
        );
    }

    /// A database holding just the translation tables, created from the
    /// migration itself so the test cannot drift away from the schema.
    async fn fixture() -> (tempfile::TempDir, sqlx::SqlitePool) {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("translations.sqlite");

        let pool = sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .expect("pool");

        sqlx::raw_sql(include_str!(
            "../migrations/20260821204218_translation_phrases.sql"
        ))
        .execute(&pool)
        .await
        .expect("schema");

        (dir, pool)
    }

    async fn stored(pool: &sqlx::SqlitePool, id: &str, lang: Lang) -> String {
        let code = lang.code();

        sqlx::query_scalar!(
            r#"SELECT translated FROM translation WHERE phrase_id = ? AND lang = ?"#,
            id,
            code
        )
        .fetch_one(pool)
        .await
        .expect("translation row")
    }

    #[tokio::test]
    async fn discovery_stocks_a_blank_row_for_every_language() {
        let (_dir, pool) = fixture().await;

        record_into(&pool, "", "Sign in", Origin::Discovered)
            .await
            .expect("record");

        let id = phrase_id("", "Sign in");

        for lang in Lang::TRANSLATABLE {
            assert_eq!(stored(&pool, &id, lang).await, "");
        }
    }

    /// The one that matters. Discovery runs every time a phrase is
    /// displayed, so if it replaced the row instead of leaving it alone, an
    /// ordinary page render would throw away a translator's work -- days
    /// after they did it, and with nothing to point at.
    #[tokio::test]
    async fn displaying_a_phrase_does_not_disturb_its_translation() {
        let (_dir, pool) = fixture().await;

        record_into(&pool, "", "Sign in", Origin::Discovered)
            .await
            .expect("first sighting");

        let id = phrase_id("", "Sign in");

        let mut phrase = Phrase::new("", "Sign in", Origin::Discovered);
        phrase
            .translations
            .insert(String::from("es"), String::from("Iniciar sesión"));
        phrase.write_into(&pool).await.expect("translate");

        let first_seen = sqlx::query_scalar!(
            r#"SELECT first_seen FROM translation_phrase WHERE phrase_id = ?"#,
            id
        )
        .fetch_one(&pool)
        .await
        .expect("phrase row");

        // The site goes on displaying the phrase.
        for _ in 0..5 {
            record_into(&pool, "", "Sign in", Origin::Discovered)
                .await
                .expect("seen again");
        }

        assert_eq!(stored(&pool, &id, Lang::Es).await, "Iniciar sesión");
        assert_eq!(stored(&pool, &id, Lang::Pt).await, "");

        let still = sqlx::query_scalar!(
            r#"SELECT first_seen FROM translation_phrase WHERE phrase_id = ?"#,
            id
        )
        .fetch_one(&pool)
        .await
        .expect("phrase row");

        assert_eq!(first_seen, still, "first_seen should not move");
    }

    #[tokio::test]
    async fn a_second_edit_replaces_the_first() {
        let (_dir, pool) = fixture().await;

        let id = phrase_id("", "Sign in");

        let mut phrase = Phrase::new("", "Sign in", Origin::Manual);
        phrase
            .translations
            .insert(String::from("es"), String::from("Entrar"));
        phrase.write_into(&pool).await.expect("first");

        phrase
            .translations
            .insert(String::from("es"), String::from("Iniciar sesión"));
        phrase.write_into(&pool).await.expect("second");

        assert_eq!(stored(&pool, &id, Lang::Es).await, "Iniciar sesión");
    }

    /// Two instances discovering the same phrase must agree it is one
    /// phrase, or the admin fills with duplicates nobody can reconcile.
    #[tokio::test]
    async fn two_instances_discovering_the_same_phrase_make_one_row() {
        let (_dir, pool) = fixture().await;

        record_into(&pool, "", "Sign in", Origin::Discovered)
            .await
            .expect("one instance");
        record_into(&pool, "", "Sign in", Origin::Content)
            .await
            .expect("another instance");

        let count = sqlx::query_scalar!(r#"SELECT COUNT(*) FROM translation_phrase"#)
            .fetch_one(&pool)
            .await
            .expect("count");

        assert_eq!(count, 1);
    }

    /// The seeded translations carry ids computed outside Rust, when the
    /// migration was written. If that derivation ever disagreed with
    /// [`phrase_id`], every seeded translation would sit in the table
    /// unreachable and the pages would quietly stay English -- with nothing
    /// failing to point at it. So the seed is loaded and checked.
    #[tokio::test]
    async fn every_seeded_phrase_is_one_the_running_code_can_find() {
        let (_dir, pool) = fixture().await;

        sqlx::raw_sql(include_str!(
            "../migrations/20260821213524_seed_login_page_translations.sql"
        ))
        .execute(&pool)
        .await
        .expect("seed");

        let rows = sqlx::query!(r#"SELECT phrase_id, context, source FROM translation_phrase"#)
            .fetch_all(&pool)
            .await
            .expect("seeded phrases");

        assert!(!rows.is_empty(), "the seed should insert something");

        for row in &rows {
            assert_eq!(
                row.phrase_id,
                phrase_id(&row.context, &row.source),
                "seeded id does not match the one {:?} would be looked up under",
                row.source
            );
        }

        // And every one of them actually says something in both languages.
        for row in &rows {
            for lang in Lang::TRANSLATABLE {
                assert!(
                    !stored(&pool, &row.phrase_id, lang).await.is_empty(),
                    "{:?} has no {} translation",
                    row.source,
                    lang.code()
                );
            }
        }
    }

    /// Seeding runs after the site may already have discovered the phrase
    /// for itself, and could run again on a rebuilt database.
    #[tokio::test]
    async fn seeding_fills_blanks_without_overwriting_anyone() {
        let (_dir, pool) = fixture().await;

        let source = "Log In or Sign Up";
        let id = phrase_id("", source);

        // The site displayed the page before the seed arrived.
        record_into(&pool, "", source, Origin::Discovered)
            .await
            .expect("discovered first");

        let seed = include_str!("../migrations/20260821213524_seed_login_page_translations.sql");

        sqlx::raw_sql(seed).execute(&pool).await.expect("seed");

        assert_eq!(
            stored(&pool, &id, Lang::Es).await,
            "Iniciar sesión o crear una cuenta"
        );

        // A translator improves on it, and the seed runs again.
        let mut phrase = Phrase::new("", source, Origin::Manual);
        phrase
            .translations
            .insert(String::from("es"), String::from("Entrar o registrarse"));
        phrase.write_into(&pool).await.expect("translator");

        sqlx::raw_sql(seed)
            .execute(&pool)
            .await
            .expect("seed again");

        assert_eq!(stored(&pool, &id, Lang::Es).await, "Entrar o registrarse");
    }

    #[tokio::test]
    async fn the_language_comes_from_the_task_it_is_asked_in() {
        assert_eq!(current_lang(), Lang::En);

        with_lang(Lang::Pt, async {
            assert_eq!(current_lang(), Lang::Pt);
        })
        .await;

        assert_eq!(current_lang(), Lang::En);
    }
}
