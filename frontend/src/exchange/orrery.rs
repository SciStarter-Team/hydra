//! Exchanges as orrery has them, carried into hydra.
//!
//! Until the cutover orrery is where exchanges are set up and moderated, so
//! everything it holds about one -- its settings, people, prompted searches
//! and squelches -- replaces hydra's copy on every pass. What only hydra has
//! (slug, description, logo, and the settings orrery has no counterpart for)
//! is kept.
//!
//! An exchange is keyed by orrery's ULID, which is also what the project
//! sync sends as a project's `exchange`, and keeps orrery's numeric id as its
//! `path_id`, so `/exchange/<id>/` addresses the same exchange on both.

use anyhow::Error;
use common::shared::db;
use common::timestring::TimeString;
use serde::Deserialize;
use ulid::Ulid;

use super::{Exchange, ExchangeSettings, HiddenProject, PredefinedSearch, SearchParams, access};
use crate::peering::LoadStorePeered;

/// One exchange, as orrery's `/staff/sync/exchanges` describes it.
#[derive(Debug, Clone, Deserialize)]
pub struct OrreryExchange {
    pub id: i64,
    pub ulid: String,
    pub name: String,
    pub region_id: Option<i64>,
    pub exclude_scistarter: bool,
    pub search: String,
    pub restricted_add: bool,
    pub global_by_default: bool,
    pub owner: Option<String>,
    pub staff: Vec<String>,
    pub include_tags: Vec<String>,
    pub include_origins: Vec<String>,
    pub excluded_origins: Vec<String>,
    pub searches: Vec<OrrerySearch>,
    pub squelches: Vec<OrrerySquelch>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrrerySearch {
    pub id: i64,
    pub label: String,
    pub phrase: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrrerySquelch {
    pub project: String,
    pub note: String,
    pub elevate: bool,
}

/// Whether something orrery holds is waiting for the exchange it was
/// submitted through to approve it -- that is, whether hydra should file it
/// as not yet reviewed.
///
/// Orrery's approval queue is everything submitted through an exchange that
/// is still unpublished and that the exchange has not rejected, rejection
/// being recorded as a `|reject|` tag.
pub fn awaiting_approval(exchange: Option<&str>, publish: bool, tags: &str) -> bool {
    exchange.is_some() && !publish && !tags.contains("|reject|")
}

/// Who is recorded as having done what this sync does: hidden a project
/// orrery squelched, since orrery does not say who did, or granted a role
/// orrery gave.
pub const BY_ORRERY: &str = "orrery";

/// The id a prompted search carried over from orrery is stored under.
///
/// Derived from orrery's own id, so a search keeps its id from pass to pass
/// and agrees between frontend instances without a mapping table. The
/// timestamp part is zero, which no ULID hydra mints can share.
pub fn search_id(orrery_id: i64) -> Ulid {
    Ulid::from_parts(0, orrery_id as u128)
}

/// An exchange search written for Postgres, rewritten for the regex crate
/// SQLite's `REGEXP` runs here.
///
/// The two dialects agree on everything the field is meant for -- words
/// and alternations -- except Postgres's word boundaries, which have their
/// own spellings there: `\m` and `[[:<:]]` for the start of a word, `\M`
/// and `[[:>:]]` for the end, `\y` and `\Y` for a boundary or not.
pub fn search_from_postgres(pattern: &str) -> String {
    let mut rewritten = String::with_capacity(pattern.len());
    let mut rest = pattern;

    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("[[:<:]]") {
            rewritten.push_str(r"\b{start}");
            rest = after;
        } else if let Some(after) = rest.strip_prefix("[[:>:]]") {
            rewritten.push_str(r"\b{end}");
            rest = after;
        } else if c == '\\' {
            let mut escape = rest[1..].chars();
            match escape.next() {
                Some('m') => rewritten.push_str(r"\b{start}"),
                Some('M') => rewritten.push_str(r"\b{end}"),
                Some('y') => rewritten.push_str(r"\b"),
                Some('Y') => rewritten.push_str(r"\B"),
                Some(other) => {
                    rewritten.push('\\');
                    rewritten.push(other);
                }
                None => rewritten.push('\\'),
            }
            rest = escape.as_str();
        } else {
            rewritten.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }

    rewritten
}

/// Hydra's exchange as orrery describes it.
///
/// `existing` is what hydra already holds for the same exchange, if
/// anything; the fields orrery has no say over are taken from it.
/// `now` stamps squelches hydra has not seen before.
///
/// An exchange whose search cannot be made to run here is refused rather
/// than carried over, since it would make every search on the exchange
/// fail.
pub fn adopt(
    orrery: &OrreryExchange,
    existing: Option<&Exchange>,
    now: &str,
) -> Result<Exchange, Error> {
    let search = search_from_postgres(&orrery.search);
    if let Some(error) = super::handlers::search_error(&search) {
        anyhow::bail!(
            "its search {:?} cannot run in hydra ({error}); change it in orrery",
            orrery.search
        );
    }

    let defaults = ExchangeSettings {
        allow_project_voting: true,
        allow_project_reviews: true,
        ..Default::default()
    };
    let kept = existing
        .map(|exchange| &exchange.settings)
        .unwrap_or(&defaults);

    let settings = ExchangeSettings {
        allow_project_voting: kept.allow_project_voting,
        allow_project_reviews: kept.allow_project_reviews,
        require_approval: kept.require_approval,
        custom_css: kept.custom_css.clone(),
        restricted_add: orrery.restricted_add,
        global_by_default: orrery.global_by_default,
        region_id: orrery.region_id,
        excluded_origins: orrery.excluded_origins.clone(),
        exclude_scistarter: orrery.exclude_scistarter,
        include_tags: orrery.include_tags.clone(),
        include_origins: orrery.include_origins.clone(),
        search,
    };

    let predefined_searches = orrery
        .searches
        .iter()
        .enumerate()
        .map(|(order, search)| PredefinedSearch {
            search_id: search_id(search.id),
            name: search.label.clone(),
            search_params: SearchParams {
                phrase: PredefinedSearch::phrases(&search.phrase),
                page: 1,
                page_size: 20,
                ..Default::default()
            },
            display_order: order as i32,
        })
        .collect();

    let mut hidden_projects = std::collections::BTreeMap::new();
    for squelch in orrery.squelches.iter() {
        let project_id = Ulid::from_string(&squelch.project)?;
        let earlier = existing.and_then(|exchange| exchange.hidden_projects.get(&project_id));

        hidden_projects.insert(
            project_id,
            HiddenProject {
                hidden_by: earlier
                    .map(|hidden| hidden.hidden_by.clone())
                    .unwrap_or_else(|| BY_ORRERY.to_string()),
                hidden_at: earlier
                    .map(|hidden| hidden.hidden_at.clone())
                    .unwrap_or_else(|| now.to_string()),
                note: squelch.note.clone(),
                elevate: squelch.elevate,
            },
        );
    }

    Ok(Exchange {
        exchange_id: Ulid::from_string(&orrery.ulid)?,
        path_id: orrery.id,
        slug: existing
            .map(|exchange| exchange.slug.clone())
            .unwrap_or_else(|| format!("exchange-{}", orrery.id)),
        name: orrery.name.clone(),
        description: existing
            .map(|exchange| exchange.description.clone())
            .unwrap_or_default(),
        logo_url: existing.and_then(|exchange| exchange.logo_url.clone()),
        settings,
        predefined_searches,
        hidden_projects,
    })
}

/// The exchange hydra holds under `exchange_id`, if any.
async fn held(exchange_id: Ulid) -> Result<Option<Exchange>, Error> {
    let exchange_id = exchange_id.to_string();
    let exists = sqlx::query_scalar!("SELECT 1 FROM exchanges WHERE exchange_id = ?", exchange_id)
        .fetch_optional(db())
        .await?
        .is_some();

    if exists {
        Ok(Some(
            Exchange::read_from_database(Ulid::from_string(&exchange_id)?).await?,
        ))
    } else {
        Ok(None)
    }
}

/// An exchange hydra made for itself under a `path_id` orrery has since
/// given a ULID of its own -- one set up by hand in hydra's admin before
/// this sync existed.
async fn stand_in(path_id: i64, exchange_id: Ulid) -> Result<Option<Exchange>, Error> {
    let exchange_id = exchange_id.to_string();
    let other = sqlx::query_scalar!(
        "SELECT exchange_id FROM exchanges WHERE path_id = ? AND exchange_id != ?",
        path_id,
        exchange_id
    )
    .fetch_optional(db())
    .await?;

    match other {
        Some(other) => Ok(Some(
            Exchange::read_from_database(Ulid::from_string(&other)?).await?,
        )),
        None => Ok(None),
    }
}

/// Bring this frontend's copy of one exchange into line with orrery.
///
/// Every frontend instance runs the sync, so this writes locally rather
/// than broadcasting. People are user-service permissions, which every
/// instance shares, and setting them is idempotent.
pub async fn apply(orrery: &OrreryExchange) -> Result<(), Error> {
    let exchange_id = Ulid::from_string(&orrery.ulid)?;
    let mut existing = held(exchange_id).await?;

    // A stand-in gives way to orrery's exchange, handing on what only hydra
    // had. Its people go with it; orrery's are set below.
    if let Some(stand_in) = stand_in(orrery.id, exchange_id).await? {
        tracing::info!(
            "Exchange {} ({}) replaces hydra's stand-in {}",
            orrery.id,
            orrery.name,
            stand_in.exchange_id
        );

        for (user_id, _) in access::people(stand_in.exchange_id).await? {
            access::revoke(stand_in.exchange_id, user_id).await?;
        }
        Exchange::delete_from_database(stand_in.exchange_id).await?;

        existing.get_or_insert(stand_in);
    }

    let adopted = adopt(orrery, existing.as_ref(), &TimeString::now().to_string())?;
    adopted.save_to_database().await?;

    let owner = orrery.owner.as_deref().map(Ulid::from_string).transpose()?;
    let staff = orrery
        .staff
        .iter()
        .map(|user_id| Ulid::from_string(user_id))
        .collect::<Result<Vec<_>, _>>()?;

    access::set_people(exchange_id, orrery.id, owner, &staff, BY_ORRERY).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const NZ: &str = "01M2MR8QB0GR6KAPXE7ZNSP63Y";
    const PROJECT: &str = "01JS1MRVX838H7WZBCCABDKW90";

    fn orrery() -> OrreryExchange {
        OrreryExchange {
            id: 38,
            ulid: NZ.into(),
            name: "New Zealand".into(),
            region_id: Some(120),
            exclude_scistarter: false,
            search: "bird|fish".into(),
            restricted_add: true,
            global_by_default: false,
            owner: None,
            staff: Vec::new(),
            include_tags: vec!["nz".into()],
            include_origins: Vec::new(),
            excluded_origins: vec!["CitSci.org".into()],
            searches: vec![OrrerySearch {
                id: 7,
                label: "Birds".into(),
                phrase: "bird, fish".into(),
            }],
            squelches: vec![OrrerySquelch {
                project: PROJECT.into(),
                note: "Australia only".into(),
                elevate: true,
            }],
        }
    }

    #[test]
    fn a_new_exchange_takes_everything_from_orrery() {
        let exchange = adopt(&orrery(), None, "2026-10-08 00:00:00").unwrap();

        assert_eq!(exchange.exchange_id.to_string(), NZ);
        assert_eq!(exchange.path_id, 38);
        assert_eq!(exchange.slug, "exchange-38");
        assert_eq!(exchange.settings.region_id, Some(120));
        assert_eq!(exchange.settings.search, "bird|fish");
        assert!(exchange.settings.restricted_add);
        assert!(!exchange.settings.global_by_default);
        assert_eq!(exchange.settings.excluded_origins, ["CitSci.org"]);
        assert_eq!(exchange.settings.include_tags, ["nz"]);
        assert!(exchange.settings.allow_project_voting);

        let search = &exchange.predefined_searches[0];
        assert_eq!(search.search_id, search_id(7));
        assert_eq!(search.search_params.phrase, ["bird, fish"]);

        let hidden = &exchange.hidden_projects[&Ulid::from_string(PROJECT).unwrap()];
        assert_eq!(hidden.hidden_by, BY_ORRERY);
        assert_eq!(hidden.hidden_at, "2026-10-08 00:00:00");
        assert_eq!(hidden.note, "Australia only");
        assert!(hidden.elevate);
    }

    #[test]
    fn what_only_hydra_has_is_kept() {
        let mut held = adopt(&orrery(), None, "2026-10-01 00:00:00").unwrap();
        held.slug = "new-zealand".into();
        held.description = "Aotearoa".into();
        held.logo_url = Some("https://example.org/logo.png".into());
        held.settings.allow_project_voting = false;
        held.settings.custom_css = Some("body {}".into());
        held.settings.search = "stale".into();

        let exchange = adopt(&orrery(), Some(&held), "2026-10-08 00:00:00").unwrap();

        assert_eq!(exchange.slug, "new-zealand");
        assert_eq!(exchange.description, "Aotearoa");
        assert_eq!(
            exchange.logo_url.as_deref(),
            Some("https://example.org/logo.png")
        );
        assert!(!exchange.settings.allow_project_voting);
        assert_eq!(exchange.settings.custom_css.as_deref(), Some("body {}"));
        assert_eq!(exchange.settings.search, "bird|fish");
    }

    #[test]
    fn a_squelch_keeps_who_and_when_but_takes_orrerys_note() {
        let project = Ulid::from_string(PROJECT).unwrap();
        let mut held = adopt(&orrery(), None, "2026-10-01 00:00:00").unwrap();
        held.hidden_projects = BTreeMap::from([(
            project,
            HiddenProject {
                hidden_by: "01K00000000000000000000USR".into(),
                hidden_at: "2026-09-01 00:00:00".into(),
                note: "old note".into(),
                elevate: false,
            },
        )]);

        let exchange = adopt(&orrery(), Some(&held), "2026-10-08 00:00:00").unwrap();
        let hidden = &exchange.hidden_projects[&project];

        assert_eq!(hidden.hidden_by, "01K00000000000000000000USR");
        assert_eq!(hidden.hidden_at, "2026-09-01 00:00:00");
        assert_eq!(hidden.note, "Australia only");
        assert!(hidden.elevate);
    }

    #[test]
    fn a_squelch_lifted_in_orrery_is_lifted_here() {
        let held = adopt(&orrery(), None, "2026-10-01 00:00:00").unwrap();
        let mut lifted = orrery();
        lifted.squelches.clear();

        let exchange = adopt(&lifted, Some(&held), "2026-10-08 00:00:00").unwrap();

        assert!(exchange.hidden_projects.is_empty());
    }

    #[test]
    fn postgres_word_boundaries_become_the_regex_crates() {
        assert_eq!(
            search_from_postgres(r"\mbirds?\M"),
            r"\b{start}birds?\b{end}"
        );
        assert_eq!(
            search_from_postgres("[[:<:]]fish[[:>:]]"),
            r"\b{start}fish\b{end}"
        );
        assert_eq!(search_from_postgres(r"\ybee\Y"), r"\bbee\B");
        assert_eq!(search_from_postgres(r"a\.b\\m|ñandú"), r"a\.b\\m|ñandú");
        assert_eq!(search_from_postgres("bird|fish"), "bird|fish");
    }

    #[test]
    fn the_test_exchanges_search_runs_and_means_the_same() {
        let mut test = orrery();
        test.search = r"\mbirds?\M".into();

        let exchange = adopt(&test, None, "2026-10-08 00:00:00").unwrap();
        let pattern = regex::Regex::new(&super::super::handlers::search_pattern(
            &exchange.settings.search,
        ))
        .unwrap();

        assert!(pattern.is_match("Backyard Birds"));
        assert!(pattern.is_match("bird count"));
        assert!(!pattern.is_match("Birdwatching"));
    }

    #[test]
    fn a_search_that_cannot_run_here_is_refused() {
        let mut broken = orrery();
        broken.search = "bird(".into();

        assert!(adopt(&broken, None, "2026-10-08 00:00:00").is_err());
    }

    #[test]
    fn only_unpublished_unrejected_exchange_submissions_await_approval() {
        assert!(awaiting_approval(Some(NZ), false, "||bird||"));
        assert!(!awaiting_approval(Some(NZ), true, "||bird||"), "published");
        assert!(!awaiting_approval(Some(NZ), false, "|reject|"), "rejected");
        assert!(
            !awaiting_approval(None, false, ""),
            "not through an exchange"
        );
    }

    #[test]
    fn search_ids_are_stable_and_distinct() {
        assert_eq!(search_id(7), search_id(7));
        assert_ne!(search_id(7), search_id(8));
    }
}
