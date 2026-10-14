//! Translation management.
//!
//! The list is stocked by the site itself: every phrase [`crate::i18n::tr`]
//! has been asked for appears here, translated or not. Phrases can also be
//! added by hand, for work done before the text goes live.

use std::collections::HashMap;

use crate::component::{AdminLayout, Component};
use crate::i18n::{Lang, Origin, Phrase, Provenance, Translated, phrase_id};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use common::shared::db;
use poem::{
    Endpoint, IntoResponse, Route, get, handler,
    web::{Form, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;

const PER_PAGE: usize = 50;

/// One row of the list.
struct PhraseRow {
    phrase_id: String,
    context: String,
    source: String,
    origin: Origin,
    last_seen: String,
    /// One entry per language a phrase can be translated into, in the order
    /// [`Lang::TRANSLATABLE`] gives them, so the cells line up with the
    /// headings the template writes from the same list. Empty text means the
    /// language is still untranslated.
    translations: Vec<(Lang, Translated)>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/translations.stpl.html")]
struct TranslationsPage {
    rows: Vec<PhraseRow>,
    search: String,
    show: Show,
    page: usize,
    per_page: usize,
    total: usize,
    outstanding_total: usize,
}

#[derive(TemplateOnce)]
#[template(path = "admin/translation_edit.stpl.html")]
struct TranslationEditPage {
    phrase: Phrase,
    languages: Vec<(Lang, Translated)>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/translation_new.stpl.html")]
struct NewPhrasePage {
    error: Option<String>,
}

/// Which phrases the list is showing.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Show {
    /// Everything.
    All,
    /// Only phrases with at least one language still untranslated or only
    /// machine translated, which is the working view.
    #[default]
    Outstanding,
}

impl Show {
    fn as_str(self) -> &'static str {
        match self {
            Show::All => "all",
            Show::Outstanding => "outstanding",
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ListQuery {
    search: Option<String>,
    show: Option<Show>,
    page: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SaveForm {
    /// One text field per language, named for its code, and for a machine
    /// translation an `approve-` checkbox named the same way, present only
    /// when ticked.
    #[serde(flatten)]
    fields: HashMap<String, String>,
}

impl SaveForm {
    fn translation(&self, lang: Lang) -> String {
        self.fields
            .get(lang.code())
            .map(|value| value.trim().to_owned())
            .unwrap_or_default()
    }

    fn approved(&self, lang: Lang) -> bool {
        self.fields
            .contains_key(&format!("approve-{}", lang.code()))
    }
}

#[derive(Debug, Deserialize)]
struct NewPhraseForm {
    context: String,
    source: String,
}

#[handler]
async fn list(
    Query(query): Query<ListQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let search = query.search.unwrap_or_default();
    let show = query.show.unwrap_or_default();
    let page = query.page.unwrap_or(1).max(1);

    let pattern = format!("%{search}%");
    let offset = ((page - 1) * PER_PAGE) as i64;
    let limit = PER_PAGE as i64;
    let outstanding_only = show == Show::Outstanding;

    // A phrase is outstanding when any language it could be translated into
    // still needs work -- including one that has no row yet, which is why
    // this counts finished rows against the language list rather than
    // looking for unfinished ones.
    let translatable = Lang::TRANSLATABLE.len() as i64;

    // One page of phrases with their translations attached, rather than a
    // page of phrases and then a read per row: at fifty rows and a language
    // apiece that was a hundred round trips to render one screen.
    let rows = sqlx::query!(
        r#"SELECT p.phrase_id AS "phrase_id!", p.context AS "context!",
                  p.source AS "source!", p.origin AS "origin!",
                  p.last_seen AS "last_seen!",
                  -- Forced nullable: the LEFT JOIN can leave these absent for
                  -- a phrase with no translation rows, which SQLite's own
                  -- type inference does not report.
                  t.lang AS "lang?", t.translated AS "translated?",
                  t.provenance AS "provenance?"
           FROM (SELECT phrase_id, context, source, origin, last_seen
                   FROM translation_phrase
                  WHERE (source LIKE ? OR context LIKE ?)
                    AND (? = 0 OR (SELECT COUNT(*) FROM translation t
                                    WHERE t.phrase_id = translation_phrase.phrase_id
                                      AND NOT t.needs_work) < ?)
                  ORDER BY last_seen DESC
                  LIMIT ? OFFSET ?) p
           LEFT JOIN translation t ON t.phrase_id = p.phrase_id
           ORDER BY p.last_seen DESC"#,
        pattern,
        pattern,
        outstanding_only,
        translatable,
        limit,
        offset,
    )
    .fetch_all(db())
    .await?;

    let total = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM translation_phrase p
           WHERE (p.source LIKE ? OR p.context LIKE ?)
             AND (? = 0 OR (SELECT COUNT(*) FROM translation t
                             WHERE t.phrase_id = p.phrase_id AND NOT t.needs_work) < ?)"#,
        pattern,
        pattern,
        outstanding_only,
        translatable,
    )
    .fetch_one(db())
    .await? as usize;

    let outstanding_total = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM translation_phrase p
           WHERE (SELECT COUNT(*) FROM translation t
                   WHERE t.phrase_id = p.phrase_id AND NOT t.needs_work) < ?"#,
        translatable,
    )
    .fetch_one(db())
    .await? as usize;

    // The join repeats a phrase once per language, so rows are folded back
    // together by id. Position is remembered rather than assumed adjacent,
    // which keeps this correct however the rows arrive.
    let mut list: Vec<PhraseRow> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();

    for row in rows {
        let position = *positions.entry(row.phrase_id.clone()).or_insert_with(|| {
            list.push(PhraseRow {
                phrase_id: row.phrase_id.clone(),
                context: row.context.clone(),
                source: row.source.clone(),
                origin: Origin::from_str(&row.origin),
                last_seen: row.last_seen.clone(),
                translations: Lang::TRANSLATABLE
                    .into_iter()
                    .map(|lang| (lang, Translated::default()))
                    .collect(),
            });

            list.len() - 1
        });

        // A phrase with no rows in `translation` at all still lists, with
        // every language outstanding, which is what the LEFT JOIN is for.
        let (Some(lang), Some(text), Some(provenance)) = (
            row.lang.as_deref().and_then(Lang::from_tag),
            row.translated,
            row.provenance,
        ) else {
            continue;
        };

        if let Some((_, cell)) = list[position]
            .translations
            .iter_mut()
            .find(|(candidate, _)| *candidate == lang)
        {
            *cell = Translated {
                text,
                provenance: Provenance::from_str(&provenance),
            };
        }
    }

    let content = TranslationsPage {
        rows: list,
        search,
        show,
        page,
        per_page: PER_PAGE,
        total,
        outstanding_total,
    };

    let prepared = AdminLayout::new("Translations", content)
        .with_sidebar_active("translations")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn edit(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let phrase = Phrase::load(id).await?;

    let content = TranslationEditPage {
        languages: Lang::TRANSLATABLE
            .into_iter()
            .map(|lang| (lang, phrase.translated(lang).cloned().unwrap_or_default()))
            .collect(),
        phrase,
    };

    let prepared = AdminLayout::new("Translate Phrase", content)
        .with_sidebar_active("translations")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn save(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
    Form(form): Form<SaveForm>,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let mut phrase = Phrase::load(id.clone()).await?;

    for lang in Lang::TRANSLATABLE {
        let before = phrase.translated(lang).cloned().unwrap_or_default();
        let after = form.translation(lang);

        let provenance = before
            .provenance
            .after_review(&before.text, &after, form.approved(lang));

        phrase.set_translation(lang, after, provenance);
    }

    // Stores locally, hands the phrase to every other frontend instance, and
    // refreshes each one's in-memory catalog on the way through.
    phrase.store().await?;

    common::service::log_activity(
        None,
        "translation-save",
        user_info.user_id,
        None,
        Some(phrase.source.clone()),
    )
    .await?;

    Ok(poem::web::Redirect::see_other("/-admin/translations").into_response())
}

#[handler]
async fn new_form(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let prepared = AdminLayout::new("Add Phrase", NewPhrasePage { error: None })
        .with_sidebar_active("translations")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn create(
    Authenticated(user_info): Authenticated,
    Form(form): Form<NewPhraseForm>,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let context = form.context.trim();
    let source = form.source.trim();

    if source.is_empty() {
        let prepared = AdminLayout::new(
            "Add Phrase",
            NewPhrasePage {
                error: Some(String::from("A phrase needs some text.")),
            },
        )
        .with_sidebar_active("translations")
        .prepare(Some(user_info))
        .await?;

        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(prepared.render_once()?));
    }

    // Entered ahead of time, so it is marked as such and keeps that mark
    // even once the site starts displaying it.
    Phrase::new(context, source, Origin::Manual).store().await?;

    Ok(poem::web::Redirect::see_other(&format!(
        "/-admin/translations/{}",
        phrase_id(context, source)
    ))
    .into_response())
}

#[handler]
async fn delete(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    Phrase::delete(id.clone()).await?;

    common::service::log_activity(
        None,
        "translation-delete",
        user_info.user_id,
        None,
        Some(id),
    )
    .await?;

    Ok(poem::Response::builder()
        .header("HX-Redirect", "/-admin/translations")
        .status(poem::http::StatusCode::OK)
        .finish())
}

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(list))
        .at("/new", get(new_form).post(create))
        .at("/:id", get(edit).post(save).delete(delete))
}
