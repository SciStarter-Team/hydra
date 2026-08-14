//! The participant-facing view of a list.
//!
//! A list is how an educator hands a set of projects to a group of people,
//! so this page is where the group actually arrives. Opening it is what
//! produces the views the list's stats are built from; without it the
//! owner's Stats View reports on nobody.

use std::sync::Arc;

use anyhow::{Error, anyhow};
use common::message::user::BasicInfo;
use common::service::log_activity;
use common::shared::db;
use common::timestring::TimeString;
use poem::{
    Endpoint, IntoResponse, Response, Route, get, handler,
    http::StatusCode,
    post,
    web::{Form, Path, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::component::{Component, Head, StandardLayout};
use crate::dashboard::EntityCardModel;
use crate::dashboard::entity_summary;
use crate::entity_list::{EntityList, EntityListEntry, EntityListIdentity, EntityListViewer};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/:key", get(page))
        .at("/:key/identify", post(identify))
        .at("/:key/stats", get(stats))
}

/// Find a list from whatever a link happens to carry.
///
/// Orrery addressed lists by integer id or slug, and teachers have been
/// handing those URLs out for years. Resolving the integer through
/// `entity_list_id_map` is what keeps those links working across the
/// cutover.
async fn resolve(key: &str) -> Result<EntityList, Error> {
    if let Ok(list_id) = key.parse::<Ulid>() {
        if let Ok(list) = EntityList::load(list_id).await {
            return Ok(list);
        }
    }

    if let Ok(orrery_id) = key.parse::<i32>() {
        let mapped = sqlx::query_scalar!(
            "SELECT list_id FROM entity_list_id_map WHERE orrery_id = ?",
            orrery_id,
        )
        .fetch_optional(db())
        .await?;

        if let Some(list_id) = mapped.and_then(|id| id.parse::<Ulid>().ok()) {
            return EntityList::load(list_id).await;
        }
    }

    EntityList::get_by_slug(key).await
}

/// Whether this visitor may open the list at all.
///
/// A public list is open to anyone, including people who are not signed in
/// — that is the point of handing the link to a class. A private one is
/// limited to the owner, the delegate, and the people it was shared with.
async fn may_view(list: &EntityList, user_info: &Option<Arc<BasicInfo>>) -> bool {
    if !list.private {
        return true;
    }

    let Some(user) = user_info else {
        return false;
    };

    if list.owner_id == user.user_id || list.delegate_id == Some(user.user_id) {
        return true;
    }

    EntityListViewer::load((list.list_id, user.user_id))
        .await
        .is_ok()
}

#[handler]
pub async fn page(
    Path(key): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let list = resolve(&key).await?;

    if !may_view(&list, &user_info).await {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }

    // Logged before rendering, and to log-service rather than to this
    // instance's database: every frontend instance keeps its own SQLite, so
    // a view recorded locally would be invisible to the instance that later
    // renders the owner's stats. Anonymous views are recorded too — the
    // owner is told how many arrived without an account.
    log_activity(
        None,
        "entity_list_view",
        user_info.as_ref().map(|user| user.user_id),
        Some(list.list_id),
        None,
    )
    .await
    .ok();

    let identity = match &user_info {
        Some(user) => EntityListIdentity::load((list.list_id, user.user_id))
            .await
            .ok(),
        None => None,
    };

    // Only worth asking someone who is signed in: a name with no account
    // behind it cannot be matched to the activity it is meant to explain.
    let ask_identity =
        user_info.is_some() && !list.hide_identity_form && identity.is_none();

    let entries = EntityListEntry::get_entries(list.list_id).await?;
    let slugs: Vec<String> = entries.iter().map(|e| e.entity_slug.clone()).collect();
    let summaries = entity_summary::lookup_batch_by_slug(&slugs).await?;

    let cards = entries
        .into_iter()
        .map(|entry| {
            let summary = summaries.iter().find(|s| s.slug == entry.entity_slug);
            ListPageEntry {
                note: entry.note,
                link: entry.link,
                card: summary.map(EntityCardModel::from_summary),
            }
        })
        .collect();

    let is_owner = user_info
        .as_ref()
        .is_some_and(|user| list.owner_id == user.user_id || list.delegate_id == Some(user.user_id));

    let title = list.name.clone();

    let content = ListPageContent {
        list_id: list.list_id,
        name: list.name,
        note_html: crate::badge::markdown_to_html(&list.note),
        sidebar: list.sidebar,
        image: Some(list.image).filter(|image| !image.is_empty()),
        entries: cards,
        ask_identity,
        identified_as: identity.map(|identity| identity.display_name()),
        signed_in: user_info.is_some(),
        is_owner,
    };

    let layout = StandardLayout::new(format!("/list/{key}"), content)
        .prepare(user_info.clone())
        .await?;

    ListPage {
        head: Head::default().prepare(None).await?,
        title,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(Debug, Deserialize)]
pub struct IdentifyForm {
    pub first_name: String,
    pub last_name: String,
}

#[handler]
pub async fn identify(
    Path(key): Path<String>,
    Authenticated(user_info): Authenticated,
    Form(form): Form<IdentifyForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let list = resolve(&key).await?;

    if !may_view(&list, &Some(user_info.clone())).await {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }

    if list.hide_identity_form {
        return Err(anyhow!("This list does not ask participants to identify"));
    }

    EntityListIdentity {
        list_id: list.list_id,
        user_id: user_info.user_id,
        first_name: form.first_name.trim().to_string(),
        last_name: form.last_name.trim().to_string(),
        given_at: TimeString::now().to_string(),
    }
    .store()
    .await?;

    Ok(Redirect::see_other(format!("/list/{key}")).into_response())
}

pub struct ListPageEntry {
    pub note: String,
    pub link: String,
    pub card: Option<EntityCardModel>,
}

#[derive(TemplateOnce)]
#[template(path = "pages/entity_list.stpl.html")]
struct ListPage {
    head: <Head as Component>::Prepared,
    title: String,
    layout: <StandardLayout<ListPageContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "pages/entity_list_content.stpl.html")]
struct ListPageContent {
    list_id: Ulid,
    name: String,
    note_html: String,
    sidebar: String,
    image: Option<String>,
    entries: Vec<ListPageEntry>,
    ask_identity: bool,
    identified_as: Option<String>,
    signed_in: bool,
    is_owner: bool,
}

// ============================================================================
// Stats View
// ============================================================================

/// What the owner of a list is shown about each participant.
///
/// The figures answer one question — what did this person do with this
/// project after the list existed — so the split is drawn against the
/// list's creation date and nothing else. Everything keyed to a different
/// date is secondary, and says which date it is keyed to.
pub struct StatsRow {
    pub participant: String,
    pub entity_name: String,
    pub entity_link: String,
    pub first_viewed: String,
    pub since_list_creation: i64,
    pub before_list_creation: i64,
    pub since_viewing: i64,
}

impl StatsRow {
    /// Prior activity plus activity since creation, which between them
    /// account for every contribution exactly once.
    pub fn total(&self) -> i64 {
        self.before_list_creation + self.since_list_creation
    }
}

/// The date out of a timestamp, which is all these figures are precise to.
fn day(timestamp: &str) -> String {
    timestamp.get(..10).unwrap_or(timestamp).to_string()
}

#[handler]
pub async fn stats(
    Path(key): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    use common::message::log::{EntityListStatsRequest, EntityListStatsResponse};
    use common::service::request;
    use std::collections::HashMap;

    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let list = resolve(&key).await?;

    // These analytics belong to whoever runs the list, and are not shared
    // with the people on it.
    let runs_the_list =
        list.owner_id == user_info.user_id || list.delegate_id == Some(user_info.user_id);

    if !runs_the_list {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }

    let entries = EntityListEntry::get_entries(list.list_id).await?;
    let slugs: Vec<String> = entries.iter().map(|e| e.entity_slug.clone()).collect();
    let summaries = entity_summary::lookup_batch_by_slug(&slugs).await?;

    let stats: EntityListStatsResponse = request(
        "log.entity_list_stats",
        EntityListStatsRequest {
            list_id: list.list_id,
            created_at: list.created_at.clone(),
            entity_ids: summaries.iter().map(|summary| summary.id).collect(),
        },
    )
    .await?;

    let contributions: HashMap<(Ulid, Ulid), _> = stats
        .contributions
        .into_iter()
        .map(|row| ((row.user_id, row.entity_id), row))
        .collect();

    // A participant is named by whatever they told this list they are
    // called, and by their handle when they told it nothing.
    let identities = EntityListIdentity::get_for_list(list.list_id).await?;
    let named: HashMap<Ulid, String> = identities
        .into_iter()
        .filter(|identity| !identity.display_name().is_empty())
        .map(|identity| (identity.user_id, identity.display_name()))
        .collect();

    let mut rows: Vec<StatsRow> = Vec::new();

    for participant in &stats.participants {
        let participant_name = match named.get(&participant.user_id) {
            Some(name) => name.clone(),
            None => crate::dashboard::lists::lookup_handle(participant.user_id).await,
        };

        // Every participant against every entity on the list, including
        // the pairs with nothing to report. A participant who did none of
        // the work is precisely the case an educator is checking for, so a
        // zero has to be visible rather than absent.
        for summary in &summaries {
            let totals = contributions.get(&(participant.user_id, summary.id));

            rows.push(StatsRow {
                participant: participant_name.clone(),
                entity_name: summary.name.clone(),
                entity_link: entity_summary::entity_link(summary),
                first_viewed: day(&participant.first_view),
                since_list_creation: totals.map(|t| t.since_list_creation).unwrap_or(0),
                before_list_creation: totals.map(|t| t.before_list_creation).unwrap_or(0),
                since_viewing: totals.map(|t| t.since_viewing).unwrap_or(0),
            });
        }
    }

    rows.sort_by(|a, b| {
        a.participant
            .cmp(&b.participant)
            .then_with(|| a.entity_name.cmp(&b.entity_name))
    });

    let title = format!("{} stats", list.name);

    let content = StatsContent {
        list_id: list.list_id,
        name: list.name,
        created_on: if list.created_at.is_empty() {
            None
        } else {
            Some(day(&list.created_at))
        },
        identified_views: stats.identified_views,
        anonymous_views: stats.anonymous_views,
        rows,
    };

    let layout = StandardLayout::new(format!("/list/{key}/stats"), content)
        .prepare(Some(user_info))
        .await?;

    StatsPage {
        head: Head::default().prepare(None).await?,
        title,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(TemplateOnce)]
#[template(path = "pages/entity_list_stats.stpl.html")]
struct StatsPage {
    head: <Head as Component>::Prepared,
    title: String,
    layout: <StandardLayout<StatsContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "pages/entity_list_stats_content.stpl.html")]
struct StatsContent {
    list_id: Ulid,
    name: String,
    created_on: Option<String>,
    identified_views: i64,
    anonymous_views: i64,
    rows: Vec<StatsRow>,
}
