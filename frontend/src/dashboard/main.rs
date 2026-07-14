use std::sync::Arc;

use anyhow::{Error, anyhow};
use common::message::dashboard::{EntitySummary, LinkToken, UserProfile};
use common::message::log::{
    UserParticipationRequest, UserParticipationResponse, UserRecentVisitsRequest,
    UserRecentVisitsResponse,
};
use common::message::user::BasicInfo;
use poem::web::{Html, Path, Query};
use poem::{IntoResponse, Response, handler, session::Session, web::RealIp};
use sailfish::TemplateOnce;
use serde::Deserialize;

use crate::component::{Component, Head, StandardLayout};
use crate::dashboard::{
    EntityCardModel, Pager, SECTION_PAGE_SIZE, clamp_page, entity_counts, entity_grid_section,
    entity_summary, fetch_bookmarked, fetch_joined, fetch_owned,
};
use crate::local_time::{TimeFormat, local_time};
use crate::middleware::authorization::Authenticated;

const RECENT_VISIT_DAYS: i64 = 30;
const RECENT_VISIT_LIMIT: i64 = 12;
const RECENT_CONTRIBUTION_DAYS: i64 = 14;

#[handler]
pub async fn page(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let user_id = user_info.user_id;
    let user_id_str = user_id.to_string();

    // The profile supplies the avatar and real name for the page header;
    // fall back to the session's basic info if it can't be fetched.
    let profile: Option<UserProfile> = common::service::request("user.profile.get", &user_id_str)
        .await
        .ok();

    let participation: UserParticipationResponse = common::service::request(
        "log.user_participation",
        UserParticipationRequest {
            user_id,
            recent_limit: 0,
            recent_days: RECENT_CONTRIBUTION_DAYS,
        },
    )
    .await
    .unwrap_or(UserParticipationResponse {
        total: 0,
        recent: vec![],
        recent_count: 0,
    });

    let (owned_projects, owned_events) = fetch_owned(&user_id_str).await?;
    let joined = fetch_joined(&user_id_str).await?;
    let (bookmarked_projects, bookmarked_events) = fetch_bookmarked(&user_id_str).await?;

    let visits: UserRecentVisitsResponse = common::service::request(
        "log.user_recent_visits",
        UserRecentVisitsRequest {
            user_id,
            days: RECENT_VISIT_DAYS,
            limit: RECENT_VISIT_LIMIT,
        },
    )
    .await
    .unwrap_or(UserRecentVisitsResponse { visits: vec![] });

    let candidate_slugs: Vec<String> = visits
        .visits
        .iter()
        .filter_map(|v| {
            let trimmed = v.path.trim_start_matches('/');
            if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains('?') {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect();
    let visited = entity_summary::lookup_batch_by_slug(&candidate_slugs).await?;
    let visited_cards: Vec<EntityCardModel> =
        visited.iter().map(EntityCardModel::from_summary).collect();

    let link_tokens: Vec<LinkToken> =
        common::service::request("user.link_tokens.list_by_user", user_id_str.clone())
            .await
            .unwrap_or_default();

    let (display_name, image) = match &profile {
        Some(profile) if !profile.first_name.is_empty() || !profile.last_name.is_empty() => (
            format!("{} {}", profile.first_name, profile.last_name)
                .trim()
                .to_string(),
            profile.image.clone(),
        ),
        Some(profile) => (profile.handle.clone(), profile.image.clone()),
        None => (user_info.handle.clone(), None),
    };

    let has_owned = !owned_projects.is_empty() || !owned_events.is_empty();
    let show_empty_state = participation.total == 0
        && !has_owned
        && joined.is_empty()
        && bookmarked_projects.is_empty()
        && bookmarked_events.is_empty();

    let token_cards: Vec<LinkTokenCardModel> = link_tokens
        .into_iter()
        .map(LinkTokenCardModel::from)
        .collect();

    let content = OverviewContent {
        display_name,
        handle: user_info.handle.clone(),
        image,
        user_id: user_id_str.clone(),
        has_owned,
        show_empty_state,
        leader: LeaderPanel {
            owned_projects_count: owned_projects.len(),
            owned_events_count: owned_events.len(),
            projects: owned_projects_section(&owned_projects, 0).await,
            events: owned_events_section(&owned_events, 0).await,
        },
        participant: ParticipantPanel {
            total_contributions: participation.total,
            recent_contributions: participation.recent_count,
            joined_count: joined.len(),
            joined: joined_section(&joined, 0),
            saves: saves_section(&bookmarked_projects, 0),
            my_events: my_events_section(&bookmarked_events, 0),
            visited: visited_cards,
        },
        link_tokens: token_cards,
    };

    let layout = StandardLayout::new("/dashboard", content)
        .prepare(Some(user_info))
        .await?;

    OverviewPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(Debug, Deserialize)]
pub struct SectionQuery {
    #[serde(default)]
    pub page: usize,
}

/// HTMX fragment endpoint: one page of a dashboard section, swapped in
/// place of the section container by the pager buttons.
#[handler]
pub async fn section(
    Path(section): Path<String>,
    Query(query): Query<SectionQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let user_id_str = user_info.user_id.to_string();
    let page = query.page;

    let html = match section.as_str() {
        "owned-projects" => {
            let (projects, _) = fetch_owned(&user_id_str).await?;
            owned_projects_section(&projects, page).await.render_once()?
        }
        "owned-events" => {
            let (_, events) = fetch_owned(&user_id_str).await?;
            owned_events_section(&events, page).await.render_once()?
        }
        "joined" => {
            let joined = fetch_joined(&user_id_str).await?;
            joined_section(&joined, page).render_once()?
        }
        "saves" => {
            let (projects, _) = fetch_bookmarked(&user_id_str).await?;
            saves_section(&projects, page).render_once()?
        }
        "my-events" => {
            let (_, events) = fetch_bookmarked(&user_id_str).await?;
            my_events_section(&events, page).render_once()?
        }
        other => return Err(anyhow!("Unknown dashboard section: {other}")),
    };

    Ok(Html(html).into_response())
}

fn joined_section(joined: &[EntitySummary], page: usize) -> crate::dashboard::EntityGridSection {
    entity_grid_section(
        joined,
        page,
        "/dashboard/section/joined",
        "dash-sec-joined",
        "You have not joined any projects via SciStarter.",
    )
}

fn saves_section(projects: &[EntitySummary], page: usize) -> crate::dashboard::EntityGridSection {
    entity_grid_section(
        projects,
        page,
        "/dashboard/section/saves",
        "dash-sec-saves",
        "You have not bookmarked any projects.",
    )
}

fn my_events_section(events: &[EntitySummary], page: usize) -> crate::dashboard::EntityGridSection {
    entity_grid_section(
        events,
        page,
        "/dashboard/section/my-events",
        "dash-sec-my-events",
        "You do not have any saved events.",
    )
}

/// One page of the detailed owned-entity rows, with the engagement
/// counts fetched only for the rows on the page.
async fn owned_rows(summaries: &[EntitySummary], page: usize, with_joins: bool) -> (Vec<OwnedEntityRow>, usize, usize) {
    let (page, pages) = clamp_page(summaries.len(), page);
    let slice: Vec<&EntitySummary> = summaries
        .iter()
        .skip(page * SECTION_PAGE_SIZE)
        .take(SECTION_PAGE_SIZE)
        .collect();
    let ids: Vec<String> = slice.iter().map(|s| s.id.to_string()).collect();

    let join_counts = if with_joins {
        entity_counts("user.joins.count_by_projects", &ids).await
    } else {
        Default::default()
    };
    let save_counts = entity_counts("user.bookmarks.count_by_projects", &ids).await;

    let rows = slice
        .into_iter()
        .map(|s| OwnedEntityRow {
            card: EntityCardModel::from_summary(s),
            joins: *join_counts.get(&s.id.to_string()).unwrap_or(&0),
            saves: *save_counts.get(&s.id.to_string()).unwrap_or(&0),
        })
        .collect();
    (rows, page, pages)
}

async fn owned_projects_section(summaries: &[EntitySummary], page: usize) -> OwnedProjectsSection {
    let (rows, page, pages) = owned_rows(summaries, page, true).await;
    OwnedProjectsSection {
        target: "dash-sec-owned-projects".to_string(),
        rows,
        pager: Pager {
            page,
            pages,
            endpoint: "/dashboard/section/owned-projects".to_string(),
            target: "dash-sec-owned-projects".to_string(),
        },
    }
}

async fn owned_events_section(summaries: &[EntitySummary], page: usize) -> OwnedEventsSection {
    let (rows, page, pages) = owned_rows(summaries, page, false).await;
    OwnedEventsSection {
        target: "dash-sec-owned-events".to_string(),
        rows,
        pager: Pager {
            page,
            pages,
            endpoint: "/dashboard/section/owned-events".to_string(),
            target: "dash-sec-owned-events".to_string(),
        },
    }
}

/// An owned project or event decorated with its engagement counts for
/// the leader panel's management rows.
pub struct OwnedEntityRow {
    pub card: EntityCardModel,
    pub joins: i64,
    pub saves: i64,
}

pub struct LinkTokenCardModel {
    pub label: String,
    pub created: String,
    pub expires: Option<String>,
}

impl From<LinkToken> for LinkTokenCardModel {
    fn from(t: LinkToken) -> Self {
        Self {
            label: t.label,
            created: t.created,
            expires: t.expires,
        }
    }
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/_partials/owned_projects_section.stpl.html")]
pub struct OwnedProjectsSection {
    target: String,
    rows: Vec<OwnedEntityRow>,
    pager: Pager,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/_partials/owned_events_section.stpl.html")]
pub struct OwnedEventsSection {
    target: String,
    rows: Vec<OwnedEntityRow>,
    pager: Pager,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/_partials/leader_panel.stpl.html")]
struct LeaderPanel {
    owned_projects_count: usize,
    owned_events_count: usize,
    projects: OwnedProjectsSection,
    events: OwnedEventsSection,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/_partials/participant_panel.stpl.html")]
struct ParticipantPanel {
    total_contributions: i64,
    recent_contributions: i64,
    joined_count: usize,
    joined: crate::dashboard::EntityGridSection,
    saves: crate::dashboard::EntityGridSection,
    my_events: crate::dashboard::EntityGridSection,
    visited: Vec<EntityCardModel>,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/main.stpl.html")]
struct OverviewPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<OverviewContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/main_content.stpl.html")]
struct OverviewContent {
    display_name: String,
    handle: String,
    image: Option<String>,
    user_id: String,
    has_owned: bool,
    show_empty_state: bool,
    leader: LeaderPanel,
    participant: ParticipantPanel,
    link_tokens: Vec<LinkTokenCardModel>,
}
