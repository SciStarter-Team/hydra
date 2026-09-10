//! Localized *presentation* of the badge pages.
//!
//! Open Badges validation reads the assertion hosted at a badge page's own
//! URL, the BadgeClass, and the metadata baked into the PNG. None of it
//! reads HTML. So a badge page can be rendered in another language without
//! touching the credential at all -- provided nothing in this module ever
//! reaches [`super::build_badge_assertion`] or [`super::bake_badge`], which
//! build all of that from [`Badge`] alone.
//!
//! That separation is the entire safety property, so it is kept at the type
//! level rather than by discipline: `Badge` is what the credential is made
//! of, and [`BadgeText`] is what a reader is shown. The conversion runs one
//! way only. [`BadgeText::shown`] renders a badge for a reader and is safe by
//! construction, because it cannot produce a `Badge` and so cannot reach the
//! credential. `BadgeText -> Badge` is the direction that would put
//! translated prose into a credential, and must never be written.
//!
//! What a badge says is translated by [`crate::i18n`], exactly like the rest
//! of the site: its name, description, criteria narrative and tags are
//! phrases, discovered when a badge page is rendered and translated in the
//! same admin as every other phrase. So nothing here enumerates which badges
//! have been translated or which languages they have been translated into --
//! any badge may have a translation in any of [`Lang::TRANSLATABLE`], or in
//! none of them, and adding one is a translator's job rather than a deploy.
//! This module holds only the two things about a badge page that the general
//! mechanism has no opinion about: what counts as the badge *being*
//! translated, and which picture to draw it on.
//!
//! A badge's own prose is all of one language or none of it: a criteria list
//! with three bullets translated and two not would read as a defect rather
//! than as a partial translation. That is the one place a badge differs from
//! ordinary page text, which falls back phrase by phrase, and it is why
//! [`Prose`] is looked up as a unit.
//!
//! Chrome -- the page talking, rather than the badge talking -- is not here
//! at all. It goes through [`crate::i18n::tr`] where it is written.

use common::badge::Badge;

use crate::i18n::{Lang, translation};

/// The context under which a badge's "learn how to earn this" address is
/// translated.
///
/// A URL is not prose, and without a context it would sit in the admin
/// looking like a phrase somebody had stocked by mistake. Naming the sense
/// says what a translator is being asked for: the address of the same
/// tutorial in their language, if there is one.
pub(super) const EARN_URL: &str = "badge: where to earn it";

/// A badge's own words, in one language.
///
/// Looked up as a unit rather than field by field, because a badge speaks a
/// language or it does not: see the module documentation.
struct Prose {
    name: String,
    description: String,
    criteria_text: String,
    tags: Vec<String>,
}

impl Prose {
    /// The badge as it is written, which is what the credential carries.
    fn english(badge: Badge) -> Prose {
        Prose {
            name: badge.name().to_owned(),
            description: badge.description().to_owned(),
            criteria_text: badge.criteria_text().to_owned(),
            tags: badge.tags().iter().map(|tag| (*tag).to_owned()).collect(),
        }
    }

    /// The badge in `lang`, or `None` unless every part of it has been
    /// translated.
    ///
    /// `None` for English too, which is not a translation but the thing
    /// translations are of.
    fn translated(badge: Badge, lang: Lang) -> Option<Prose> {
        Some(Prose {
            name: translation("", badge.name(), lang)?,
            description: translation("", badge.description(), lang)?,
            criteria_text: translation("", badge.criteria_text(), lang)?,
            tags: badge
                .tags()
                .iter()
                .map(|tag| translation("", tag, lang))
                .collect::<Option<Vec<String>>>()?,
        })
    }
}

/// Whether the badge itself can be read in `lang`.
///
/// The question the language picker asks. Answered by looking rather than by
/// consulting a list, so a badge starts offering a language the moment its
/// last phrase is translated.
fn speaks(badge: Badge, lang: Lang) -> bool {
    Prose::translated(badge, lang).is_some()
}

/// The artwork a reader of `lang` is shown, as a key into the embedded
/// [`crate::Static`] assets rather than a path on disk.
///
/// Translated artwork is found by name rather than registered: the badge's
/// own image with the language code appended to its stem. Drawing
/// `badge-data-ethics-pt.png` and committing it is therefore the whole of
/// giving that badge Portuguese artwork -- there is no second place to
/// remember, and nothing that has to be kept in step with the list of
/// languages. A badge with no picture in the reader's language is drawn on
/// its own, which is what every badge did before any of them were
/// translated.
///
/// Artwork is presentation, like the prose around it, and is chosen on its
/// own terms: the credential baked into a translated image is the same
/// canonical English payload, and the image URL recorded *in* that payload is
/// the badge's own, so a verifier never sees a translated one.
pub fn badge_artwork(badge: Badge, lang: Lang) -> String {
    let own = badge.template_path();

    if lang == Lang::En {
        return own.to_owned();
    }

    let Some((stem, extension)) = own.rsplit_once('.') else {
        return own.to_owned();
    };

    let translated = format!("{stem}-{}.{extension}", lang.code());

    if crate::Static::get(&translated).is_some() {
        translated
    } else {
        own.to_owned()
    }
}

/// Where "learn how to earn this badge" goes for a reader of `lang`.
///
/// Presentation only: the `criteria` URL recorded in the credential stays the
/// canonical English one, so the badge never points at two different things.
///
/// Falls back on its own rather than with the prose. A tutorial exists in the
/// languages somebody has written it in, which need not be the languages the
/// badge has been translated into, and pointing a Spanish reader at the
/// English tutorial is better than pointing them nowhere.
fn earn_url(badge: Badge, lang: Lang) -> Option<String> {
    let english = badge.earn_url()?;

    Some(translation(EARN_URL, english, lang).unwrap_or_else(|| english.to_owned()))
}

/// A badge's prose in one language, for display only.
///
/// Made *from* a [`Badge`] and never turned back into one: see the module
/// documentation for why that asymmetry is the point.
pub struct BadgeText {
    /// Whether the words below are a translation or the credential's own.
    ///
    /// False for English, and for a language nobody has finished translating
    /// this badge into -- such a page renders in translated chrome around the
    /// English original, which is honest, rather than half in each language.
    pub translated: bool,
    /// The badge's name as its translators render it. This is the page's
    /// headline in that language; the canonical name is still what a verifier
    /// matches against the baked image and what the "add to LinkedIn" link
    /// writes into a profile, and is one click away through the
    /// courtesy-translation notice.
    pub name: String,
    pub description: String,
    pub criteria_text: String,
    pub tags: Vec<String>,
    pub earn_url: Option<String>,
    /// The artwork for this language, which is the badge's own whenever
    /// nobody has drawn a translated one.
    pub artwork: String,
}

impl BadgeText {
    /// What a reader of `lang` is shown.
    ///
    /// The one place the fallback from "no translation" to "the credential's
    /// own words" is decided, rather than field by field at every use. In
    /// English that fallback is all there is: the badge as it is written.
    pub fn shown(badge: Badge, lang: Lang) -> BadgeText {
        let translated = Prose::translated(badge, lang);
        let is_translated = translated.is_some();

        let Prose {
            name,
            description,
            criteria_text,
            tags,
        } = translated.unwrap_or_else(|| Prose::english(badge));

        BadgeText {
            translated: is_translated,
            name,
            description,
            criteria_text,
            tags,
            earn_url: earn_url(badge, lang),
            artwork: badge_artwork(badge, lang),
        }
    }
}

/// One entry in a badge page's language picker.
pub struct LanguageLink {
    pub endonym: &'static str,
    pub code: &'static str,
    pub url: String,
    pub current: bool,
}

/// The languages a badge page can be read in, as links that pin the choice.
///
/// English and every language this badge has actually been translated into,
/// which is however many that happens to be. `?lang=` sticks for the rest of
/// the session, so these are ordinary links to the page itself;
/// [`crate::middleware::language::LANG_DEFAULT`] is offered alongside them by
/// the template, as the way back to the browser's own preference.
///
/// Empty for a badge nobody has translated: the picker's promise is a
/// translated *badge*, and the site's own language switching, which is what
/// still puts translated chrome around such a page, is not this control's to
/// offer.
pub fn language_links(badge: Badge, canonical_url: &str, current: Lang) -> Vec<LanguageLink> {
    let spoken: Vec<Lang> = Lang::TRANSLATABLE
        .into_iter()
        .filter(|lang| speaks(badge, *lang))
        .collect();

    if spoken.is_empty() {
        return Vec::new();
    }

    std::iter::once(Lang::En)
        .chain(spoken)
        .map(|lang| LanguageLink {
            endonym: lang.endonym(),
            code: lang.code(),
            url: format!("{canonical_url}?lang={}", lang.code()),
            current: lang == current,
        })
        .collect()
}

/// The `?lang=` a badge page appends to the URLs it points at its own image,
/// so that a reader downloads the artwork they were shown.
///
/// Empty for English, whose artwork is what those URLs serve by default. The
/// page has to say this explicitly rather than letting the image handler
/// consult the session, because the URL it is saying it about is also the
/// one the credential names.
pub fn image_query(lang: Lang) -> String {
    match lang {
        Lang::En => String::new(),
        _ => format!("?lang={}", lang.code()),
    }
}

/// The address of the English original, for the courtesy-translation notice.
pub fn english_url(canonical_url: &str) -> String {
    format!("{canonical_url}?lang={}", Lang::En.code())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::i18n::{Origin, Phrase, install_for_test, phrase_id};

    /// A badge whose phrases no other test translates.
    ///
    /// The catalog these tests install into is the process-wide one, so a
    /// test that translated a badge sharing a tag -- and most of them share
    /// several -- would change what another test sees. The One Million Acts
    /// badges carry no tags and no `earn_url`, and their names, descriptions
    /// and criteria are their own, so each of them can be translated in
    /// isolation.
    const TRANSLATED: Badge = Badge::OneMillionActsOfScience2024;
    const UNTRANSLATED: Badge = Badge::OneMillionActsOfScience2025;

    /// Translate one phrase into each of `langs` in the catalog the site
    /// translates against, the way the admin would.
    ///
    /// A phrase is stored whole -- a [`Phrase`] carries every language at
    /// once -- so all the languages it should be readable in have to be
    /// given together: installing Spanish on its own would take Portuguese
    /// back off again.
    fn install(context: &str, source: &str, langs: &[Lang], text: impl Fn(Lang) -> String) {
        let mut phrase = Phrase::new(context, source, Origin::Manual);

        for lang in langs {
            phrase
                .translations
                .insert(lang.code().to_owned(), text(*lang));
        }

        install_for_test(&phrase);
    }

    /// Everything [`Prose::translated`] asks for, in each of `langs`, marked
    /// with the language so a test can tell a translation from the original
    /// at a glance.
    fn translate_prose(badge: Badge, langs: &[Lang]) {
        let mark = |lang: Lang, source: &str| format!("<{}>{source}", lang.code());

        for source in [badge.name(), badge.description(), badge.criteria_text()] {
            install("", source, langs, |lang| mark(lang, source));
        }

        for tag in badge.tags() {
            install("", tag, langs, |lang| mark(lang, tag));
        }
    }

    #[test]
    fn the_english_rendering_is_the_credentials_own_words() {
        for badge in Badge::ALL {
            let english = BadgeText::shown(badge, Lang::En);

            assert!(!english.translated);
            assert_eq!(english.name, badge.name());
            assert_eq!(english.description, badge.description());
            assert_eq!(english.criteria_text, badge.criteria_text());
            assert_eq!(english.tags, badge.tags());
            assert_eq!(english.earn_url.as_deref(), badge.earn_url());
            assert_eq!(english.artwork, badge.template_path());
        }
    }

    /// A badge with nothing translated reads as itself in every language, so
    /// that asking for a language the badge does not speak is honest rather
    /// than broken.
    #[test]
    fn an_untranslated_badge_reads_as_itself_in_every_language() {
        for lang in Lang::ALL {
            let shown = BadgeText::shown(UNTRANSLATED, lang);

            assert!(!shown.translated);
            assert_eq!(shown.name, UNTRANSLATED.name());
            assert_eq!(shown.criteria_text, UNTRANSLATED.criteria_text());
        }
    }

    /// The picker offers a translated badge, so a badge nobody has
    /// translated shows none -- while the site's own language switching,
    /// which is what a `?lang=` still does everywhere else, is untouched.
    #[test]
    fn only_a_translated_badge_offers_a_picker() {
        assert!(
            language_links(UNTRANSLATED, "https://example.org/badges/x", Lang::En).is_empty(),
            "a badge nobody has translated offers a picker"
        );
    }

    /// The point of the rewrite: a badge is translated into whichever
    /// languages somebody has translated it into, one at a time, and the
    /// picker offers exactly those alongside the original.
    #[test]
    fn a_badge_is_offered_in_the_languages_it_has_been_translated_into() {
        let links = |current| language_links(TRANSLATED, "https://example.org/badges/x", current);

        assert!(links(Lang::En).is_empty());

        translate_prose(TRANSLATED, &[Lang::Pt]);

        let offered: Vec<&str> = links(Lang::Pt).iter().map(|link| link.code).collect();

        assert_eq!(offered, ["en", "pt"], "Spanish is not translated yet");

        let current: Vec<&str> = links(Lang::Pt)
            .iter()
            .filter(|link| link.current)
            .map(|link| link.code)
            .collect();

        assert_eq!(current, ["pt"]);

        translate_prose(TRANSLATED, &[Lang::Es, Lang::Pt]);

        let offered: Vec<&str> = links(Lang::En).iter().map(|link| link.code).collect();

        assert_eq!(offered, ["en", "es", "pt"]);
    }

    /// A badge translated into one language is shown in it, and still reads
    /// as itself in a language it has not been translated into.
    #[test]
    fn a_translation_is_shown_only_in_its_own_language() {
        let badge = Badge::OneMillionActsOfScience2026;

        translate_prose(badge, &[Lang::Es]);

        let spanish = BadgeText::shown(badge, Lang::Es);

        assert!(spanish.translated);
        assert_eq!(spanish.name, format!("<es>{}", badge.name()));
        assert_eq!(spanish.description, format!("<es>{}", badge.description()));

        let portuguese = BadgeText::shown(badge, Lang::Pt);

        assert!(!portuguese.translated);
        assert_eq!(portuguese.name, badge.name());
    }

    /// Half a badge is not a translation. A page showing a translated name
    /// over an English criteria list would read as a defect, so the whole
    /// page stays in the original until the last phrase is done.
    #[test]
    fn a_partly_translated_badge_is_not_translated() {
        let badge = Badge::IdealParticipatoryScience;

        install("", badge.name(), &[Lang::Es], |_| {
            String::from("Ciencia participativa IDEAL")
        });
        install("", badge.description(), &[Lang::Es], |_| {
            String::from("Una descripción.")
        });

        let shown = BadgeText::shown(badge, Lang::Es);

        assert!(!shown.translated, "criteria are still untranslated");
        assert_eq!(shown.name, badge.name());

        install("", badge.criteria_text(), &[Lang::Es], |_| {
            String::from("Los criterios, en español.")
        });

        assert!(BadgeText::shown(badge, Lang::Es).translated);
    }

    /// The tutorial is a separate thing from the badge, so its address
    /// follows whether *it* has been translated rather than whether the
    /// badge has.
    #[test]
    fn the_tutorial_address_falls_back_on_its_own() {
        let badge = Badge::SciStarterAmbassador;
        let english = badge.earn_url().expect("this badge has a tutorial");

        assert_eq!(
            BadgeText::shown(badge, Lang::Pt).earn_url.as_deref(),
            Some(english),
            "an untranslated tutorial keeps its English address"
        );

        install(EARN_URL, english, &[Lang::Pt], |_| {
            String::from("https://example.org/pt")
        });

        assert_eq!(
            BadgeText::shown(badge, Lang::Pt).earn_url.as_deref(),
            Some("https://example.org/pt"),
            "a badge that is not translated can still have a translated tutorial"
        );
    }

    /// Artwork is registered nowhere: it is the badge's own image with the
    /// language appended, if that file was deployed. So this checks the
    /// convention against what is actually embedded -- a badge with a
    /// translated picture gets it, and one without falls back rather than
    /// serving a 404's worth of nothing.
    #[test]
    fn translated_artwork_is_used_where_it_exists() {
        for badge in Badge::ALL {
            assert_eq!(badge_artwork(badge, Lang::En), badge.template_path());

            for lang in Lang::TRANSLATABLE {
                let artwork = badge_artwork(badge, lang);

                assert!(
                    crate::Static::get(&artwork).is_some(),
                    "no embedded artwork at {artwork}"
                );

                let (stem, extension) = badge
                    .template_path()
                    .rsplit_once('.')
                    .expect("badge artwork has an extension");
                let translated = format!("{stem}-{}.{extension}", lang.code());

                if crate::Static::get(&translated).is_some() {
                    assert_eq!(artwork, translated);
                } else {
                    assert_eq!(artwork, badge.template_path());
                }
            }
        }
    }

    /// The badge the seed migration translates really does have artwork in
    /// both of its languages, which is what a picker offering them promises.
    #[test]
    fn the_translated_badge_has_its_own_artwork_in_each_language() {
        let badge = Badge::DataEthicsForPractitioners;

        for lang in Lang::TRANSLATABLE {
            assert_ne!(
                badge_artwork(badge, lang),
                badge.template_path(),
                "{} has no artwork of its own",
                lang.code()
            );
        }
    }

    #[test]
    fn a_translated_page_points_its_images_at_its_own_language() {
        assert_eq!(image_query(Lang::En), "");
        assert_eq!(image_query(Lang::Es), "?lang=es");
        assert_eq!(image_query(Lang::Pt), "?lang=pt");
    }

    /// The seed migration carries wording that used to be Rust, so it names
    /// the badge's strings from outside the compiler's reach. These tests
    /// are what keeps that from going stale silently: edit the badge's
    /// description in `common/src/badge` and its phrase id moves, leaving
    /// the seeded translation stranded and the page quietly English.
    mod seed {
        use super::*;

        const SEED: &str =
            include_str!("../../migrations/20260908234722_seed_badge_translations.sql");

        /// Applied on top of [`SEED`], as it is in a running database: it
        /// seeds the translated month names and date pattern, and edits the
        /// badge's own criteria narrative.
        const DATES_AND_CITATION_NOTE: &str =
            include_str!("../../migrations/20260910021818_translated_dates_and_citation_note.sql");

        /// The badge the seed translates, and the languages it translates it
        /// into.
        const SEEDED: Badge = Badge::DataEthicsForPractitioners;

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
                "../../migrations/20260821204218_translation_phrases.sql"
            ))
            .execute(&pool)
            .await
            .expect("schema");

            sqlx::raw_sql(SEED).execute(&pool).await.expect("seed");
            sqlx::raw_sql(DATES_AND_CITATION_NOTE)
                .execute(&pool)
                .await
                .expect("dates and citation note");

            (dir, pool)
        }

        #[tokio::test]
        async fn every_seeded_phrase_is_one_the_running_code_can_find() {
            let (_dir, pool) = fixture().await;

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
        }

        /// The one that matters: every phrase the badge is looked up by is
        /// one the seed actually translated. A wording change in
        /// `common/src/badge` fails here rather than in production.
        #[tokio::test]
        async fn the_seeded_badge_is_translated_into_both_languages() {
            let (_dir, pool) = fixture().await;

            let mut phrases: Vec<(&str, &str)> = vec![
                ("", SEEDED.name()),
                ("", SEEDED.description()),
                ("", SEEDED.criteria_text()),
                (EARN_URL, SEEDED.earn_url().expect("a tutorial")),
            ];

            phrases.extend(SEEDED.tags().iter().map(|tag| ("", *tag)));

            for (context, source) in phrases {
                let id = phrase_id(context, source);

                for lang in Lang::TRANSLATABLE {
                    let code = lang.code();

                    let translated = sqlx::query_scalar!(
                        r#"SELECT translated FROM translation WHERE phrase_id = ? AND lang = ?"#,
                        id,
                        code
                    )
                    .fetch_optional(&pool)
                    .await
                    .expect("query")
                    .unwrap_or_default();

                    assert!(
                        !translated.is_empty(),
                        "the seed has no {code} for {source:?}"
                    );
                }
            }
        }

        /// Every month the date formatter can ask for is one the seed
        /// translated, in the context it asks under. A month left to fall
        /// back on its source would put one English word in the middle of an
        /// otherwise Spanish date.
        #[tokio::test]
        async fn every_month_the_formatter_asks_for_is_translated() {
            let (_dir, pool) = fixture().await;

            for source in crate::i18n::MONTHS
                .iter()
                .copied()
                .chain(std::iter::once("{day} {month} {year}"))
            {
                let context = if source.starts_with('{') {
                    "date"
                } else {
                    "month"
                };
                let id = phrase_id(context, source);

                for lang in Lang::TRANSLATABLE {
                    let code = lang.code();

                    let translated = sqlx::query_scalar!(
                        r#"SELECT translated FROM translation WHERE phrase_id = ? AND lang = ?"#,
                        id,
                        code
                    )
                    .fetch_optional(&pool)
                    .await
                    .expect("query")
                    .unwrap_or_default();

                    assert!(
                        !translated.is_empty(),
                        "the seed has no {code} for {source:?}"
                    );
                }
            }
        }

        /// The translations said the bibliography was quoted in its published
        /// language; the English original never did. Checked on the stored
        /// text rather than on a rendered page, because that is what the edit
        /// touched.
        #[tokio::test]
        async fn the_translated_criteria_no_longer_explain_the_citations() {
            let (_dir, pool) = fixture().await;

            let id = phrase_id("", SEEDED.criteria_text());

            for (code, sentence) in [
                ("es", "Las referencias se citan en su idioma original."),
                ("pt", "As referências são citadas no idioma original."),
            ] {
                let translated = sqlx::query_scalar!(
                    r#"SELECT translated FROM translation WHERE phrase_id = ? AND lang = ?"#,
                    id,
                    code
                )
                .fetch_one(&pool)
                .await
                .expect("a translation");

                assert!(
                    !translated.contains(sentence),
                    "{code} still carries the citation note"
                );
                // The paragraph it was appended to has to survive the edit.
                assert!(
                    translated.contains("1835352"),
                    "{code} lost the grant it introduces"
                );
            }
        }
    }
}
