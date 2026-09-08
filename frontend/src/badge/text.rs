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
//! way only. `Badge -> BadgeText` is the English rendering of a badge and is
//! safe by construction, because it cannot produce a `Badge` and so cannot
//! reach the credential; it exists so that the fallback from "no
//! translation" to "the credential's own words" is written once rather than
//! field by field at every use. `BadgeText -> Badge` is the direction that
//! would put translated prose into a credential, and must never be written.
//!
//! Chrome -- the page talking, rather than the badge talking -- is not here.
//! It goes through [`crate::i18n::tr`] like the rest of the site, so a badge
//! page is translated by the same mechanism and in the same admin as every
//! other page.
//!
//! A badge's own prose is a whole [`BadgeText`] or nothing: a criteria list
//! with three bullets translated and two not would read as a defect rather
//! than as a partial translation.

use common::badge::Badge;

use crate::i18n::Lang;

/// The badge image a reader of one language is shown, and the template a
/// download from their page is baked from.
///
/// Artwork is presentation, like the prose around it: the credential baked
/// into a translated image is the same canonical English payload, and the
/// image URL recorded *in* that payload is the badge's own, so a verifier
/// never sees a translated one.
#[derive(Clone, Copy)]
pub struct Artwork {
    /// Where the image is published.
    pub url: &'static str,
    /// Where the bake reads it from, as a key into the embedded
    /// [`crate::Static`] assets rather than a path on disk.
    pub path: &'static str,
}

/// A badge's prose in one language, for display only.
///
/// Convertible *from* [`Badge`] and never *to* it: see the module
/// documentation for why that asymmetry is the point.
#[derive(Clone, Copy)]
pub struct BadgeText {
    /// The badge's name as the translators of its tutorial render it. Shown
    /// beneath the canonical name, never in place of it -- the canonical
    /// name is what a verifier matches against the baked image and what the
    /// "add to LinkedIn" link writes into a profile.
    pub name: &'static str,
    pub description: &'static str,
    pub criteria_text: &'static str,
    pub tags: &'static [&'static str],
    /// Where "learn how to earn this badge" goes for a reader of this
    /// language. Presentation only: the `criteria` URL recorded in the
    /// credential stays the canonical English one, so the badge never
    /// points at two different things.
    pub earn_url: Option<&'static str>,
    /// The artwork for this language, which is the badge's own whenever
    /// nobody has drawn a translated one.
    pub artwork: Artwork,
}

impl From<Badge> for BadgeText {
    /// A badge rendered in the language it was written in, which is to say
    /// its own text, unchanged.
    ///
    /// This is what a page falls back to when nobody has translated the
    /// badge into the language being asked for, so that the fallback is one
    /// decision in one place instead of one per field per page.
    fn from(badge: Badge) -> BadgeText {
        BadgeText {
            name: badge.name(),
            description: badge.description(),
            criteria_text: badge.criteria_text(),
            tags: badge.tags(),
            earn_url: badge.earn_url(),
            artwork: Artwork {
                url: badge.template_url(),
                path: badge.template_path(),
            },
        }
    }
}

/// A badge's prose in every language it has been translated into.
///
/// A whole struct, so a badge is translated into all of them or none.
/// Offering a reader a choice of three languages and then quietly handing
/// one of them the English original would be worse than not offering it.
struct Translations {
    es: BadgeText,
    pt: BadgeText,
}

/// The translated prose for a badge, if there is any.
///
/// `None` for English, whose text is the credential's own, and for any badge
/// nobody has translated: such a page renders in translated chrome around
/// the English original, which is honest, rather than half in each language.
pub fn badge_text(badge: Badge, lang: Lang) -> Option<&'static BadgeText> {
    let translations = translations(badge)?;

    match lang {
        Lang::En => None,
        Lang::Es => Some(&translations.es),
        Lang::Pt => Some(&translations.pt),
    }
}

/// The artwork to show a reader of `lang`, which is the badge's own unless
/// the badge has been translated into that language.
///
/// The same fallback [`badge_text`] gets from the `Badge -> BadgeText`
/// conversion, for the image handlers, which serve bytes rather than a page
/// and so never build a [`BadgeText`].
pub fn badge_artwork(badge: Badge, lang: Lang) -> Artwork {
    badge_text(badge, lang)
        .map(|text| text.artwork)
        .unwrap_or_else(|| BadgeText::from(badge).artwork)
}

fn translations(badge: Badge) -> Option<&'static Translations> {
    match badge {
        Badge::DataEthicsForPractitioners => Some(&DATA_ETHICS),
        _ => None,
    }
}

// Wording follows the published Spanish and Portuguese tutorials
// (scistarter.org/training-dataethics-es and -pt), which were translated in
// collaboration with WCS, the Alianza Aguas Amazónicas, the Rede Brasileira
// de Ciência Cidadã and the Red Iberoamericana de Ciencia Participativa. The
// skills-and-competencies list on those pages is the same list as the
// badge's, so an earner reads the same words in both places.
//
// The same text is in orrery's `src/badgetext.rs`, which serves the same
// badge on scistarter.org: the two sites have to say the same thing about
// the same credential.
static DATA_ETHICS: Translations = Translations {
    es: BadgeText {
        name: "Ética de datos para profesionales",
        description: "La insignia “Ética de datos para profesionales” se otorga a profesionales y líderes de proyectos de ciencia participativa, incluidos proyectos de ciencia ciudadana y ciencia comunitaria, que han demostrado la capacidad de “pensar como una persona experta en ética”.",
        criteria_text: include_str!("data_ethics.es.md"),
        tags: &[
            "desarrollo profesional",
            "participación comunitaria",
            "ciencia ciudadana",
        ],
        earn_url: Some("https://scistarter.org/training-dataethics-es"),
        artwork: Artwork {
            url: concat!(
                "https://",
                env!("DOMAIN"),
                "/-static/img/badges/badge-data-ethics-es.png"
            ),
            path: "img/badges/badge-data-ethics-es.png",
        },
    },
    pt: BadgeText {
        name: "Ética de Dados para Profissionais",
        description: "O badge “Ética de Dados para Profissionais” é concedido a profissionais e lideranças de projetos de ciência participativa, incluindo projetos de ciência cidadã e ciência comunitária, que demonstraram a capacidade de “pensar como uma pessoa especialista em ética”.",
        criteria_text: include_str!("data_ethics.pt.md"),
        tags: &[
            "desenvolvimento profissional",
            "engajamento comunitário",
            "ciência cidadã",
        ],
        earn_url: Some("https://scistarter.org/training-dataethics-pt"),
        artwork: Artwork {
            url: concat!(
                "https://",
                env!("DOMAIN"),
                "/-static/img/badges/badge-data-ethics-pt.png"
            ),
            path: "img/badges/badge-data-ethics-pt.png",
        },
    },
};

/// One entry in a badge page's language picker.
pub struct LanguageLink {
    pub endonym: &'static str,
    pub code: &'static str,
    pub url: String,
    pub current: bool,
}

/// The languages a badge page can be read in, as links that pin the choice.
///
/// `?lang=` sticks for the rest of the session, so these are ordinary links
/// to the page itself; [`crate::middleware::language::LANG_DEFAULT`] is
/// offered alongside them by the template, as the way back to the browser's
/// own preference.
///
/// Empty for a badge nobody has translated: the picker's promise is a
/// translated *badge*, and the site's own language switching, which is what
/// still puts translated chrome around such a page, is not this control's
/// to offer.
pub fn language_links(badge: Badge, canonical_url: &str, current: Lang) -> Vec<LanguageLink> {
    if translations(badge).is_none() {
        return Vec::new();
    }

    Lang::ALL
        .into_iter()
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

    #[test]
    fn the_english_rendering_is_the_credentials_own_words() {
        for badge in Badge::ALL {
            let english = BadgeText::from(badge);

            assert_eq!(english.name, badge.name());
            assert_eq!(english.description, badge.description());
            assert_eq!(english.criteria_text, badge.criteria_text());
            assert_eq!(english.tags, badge.tags());
            assert_eq!(english.earn_url, badge.earn_url());
            assert_eq!(english.artwork.url, badge.template_url());
            assert_eq!(english.artwork.path, badge.template_path());
        }
    }

    /// The picker offers a translated badge, so a badge nobody has
    /// translated shows none -- while the site's own language switching,
    /// which is what a `?lang=` still does everywhere else, is untouched.
    #[test]
    fn only_a_translated_badge_offers_a_picker() {
        for badge in Badge::ALL {
            let links = language_links(badge, "https://example.org/badges/x", Lang::En);

            if translations(badge).is_some() {
                assert_eq!(links.len(), Lang::ALL.len());
            } else {
                assert!(links.is_empty(), "{} offers a picker", badge.name());
            }
        }
    }

    #[test]
    fn a_translation_says_something_different_from_the_original() {
        for badge in Badge::ALL {
            let Some(translations) = translations(badge) else {
                continue;
            };

            for text in [&translations.es, &translations.pt] {
                assert_ne!(text.name, badge.name());
                assert_ne!(text.description, badge.description());
                assert_ne!(text.criteria_text, badge.criteria_text());
            }

            assert_ne!(translations.es.name, translations.pt.name);
        }
    }

    /// English is the language the credential is written in, so it never has
    /// a translation of its own to hand back.
    #[test]
    fn english_has_no_translation() {
        for badge in Badge::ALL {
            assert!(badge_text(badge, Lang::En).is_none());
        }
    }

    /// Artwork is registered as a URL and a path to the same picture, in two
    /// different places, so the pair is checked rather than trusted: a
    /// mismatch would publish one image and bake onto another, and nothing
    /// else would notice.
    ///
    /// The two are required to share a filename only for translated artwork,
    /// which is named here. Some of the badges' own images were published
    /// under one name and committed under another long before this module
    /// existed, and renaming a published badge image is not something a test
    /// should demand.
    #[test]
    fn every_artwork_is_embedded() {
        fn basename(location: &str) -> &str {
            location.rsplit('/').next().unwrap_or(location)
        }

        for badge in Badge::ALL {
            let mut artworks = vec![BadgeText::from(badge).artwork];

            if let Some(translations) = translations(badge) {
                for text in [&translations.es, &translations.pt] {
                    assert_eq!(
                        basename(text.artwork.url),
                        basename(text.artwork.path),
                        "artwork published at {} is baked from {}",
                        text.artwork.url,
                        text.artwork.path
                    );

                    artworks.push(text.artwork);
                }
            }

            for artwork in artworks {
                assert!(
                    crate::Static::get(artwork.path).is_some(),
                    "no embedded bake template at {}",
                    artwork.path
                );
            }
        }
    }

    /// A language whose artwork was never drawn falls back to the badge's
    /// own, and one whose artwork was drawn gets it.
    #[test]
    fn translated_artwork_is_used_where_it_exists() {
        for badge in Badge::ALL {
            assert_eq!(badge_artwork(badge, Lang::En).path, badge.template_path());

            let Some(translations) = translations(badge) else {
                for lang in Lang::TRANSLATABLE {
                    assert_eq!(badge_artwork(badge, lang).path, badge.template_path());
                }

                continue;
            };

            assert_eq!(
                badge_artwork(badge, Lang::Es).path,
                translations.es.artwork.path
            );
            assert_eq!(
                badge_artwork(badge, Lang::Pt).path,
                translations.pt.artwork.path
            );
            assert_ne!(translations.es.artwork.path, translations.pt.artwork.path);
        }
    }

    #[test]
    fn a_translated_page_points_its_images_at_its_own_language() {
        assert_eq!(image_query(Lang::En), "");
        assert_eq!(image_query(Lang::Es), "?lang=es");
        assert_eq!(image_query(Lang::Pt), "?lang=pt");
    }
}
