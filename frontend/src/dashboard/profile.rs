use std::sync::Arc;

use anyhow::Error;
use common::message::dashboard::{EntitySummary, UserPlace, UserProfile};
use common::message::user::{
    BasicInfo, CertificateInfo, UserBadge, UserCertificatesRequest, UserCertificatesResponse,
};
use poem::web::{Html, Path, Query};
use poem::{IntoResponse, Response, handler, session::Session, web::RealIp};
use sailfish::TemplateOnce;

use crate::component::{Component, Head, StandardLayout};
use crate::dashboard::interests::{InterestKind, labels_for};
use crate::dashboard::main::SectionQuery;
use crate::dashboard::{
    EntityCardModel, EntityGridSection, Pager, SECTION_PAGE_SIZE, clamp_page, entity_grid_section,
    fetch_bookmarked, fetch_joined, fetch_owned,
};
use crate::entity_list::EntityList;
use crate::middleware::authorization::Authenticated;
use crate::profile_card::ProfileCard;

#[handler]
pub async fn page(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;
    let user_id = user_info.user_id;
    let user_id_str = user_id.to_string();

    let profile: UserProfile =
        common::service::request("user.profile.get", user_id_str.clone()).await?;

    // Everything beyond the core profile defaults to empty if its
    // service is unreachable, so the page still renders.
    let badges: Vec<UserBadge> = common::service::request("user.badges.all", user_id)
        .await
        .unwrap_or_default();
    let certificates: UserCertificatesResponse = common::service::request(
        "user.certificates.list",
        UserCertificatesRequest { user_id },
    )
    .await
    .unwrap_or(UserCertificatesResponse {
        certificates: vec![],
    });

    let tags: Vec<String> = common::service::request("user.tags.get", user_id_str.clone())
        .await
        .unwrap_or_default();

    let places: Vec<UserPlace> =
        common::service::request("user.places.list_by_user", user_id_str.clone())
            .await
            .unwrap_or_default();

    let lists = EntityList::get_user_lists(user_id)
        .await
        .unwrap_or_default();

    let (owned_projects, owned_events) = fetch_owned(&user_id_str).await?;
    let joined = fetch_joined(&user_id_str).await?;
    let (bookmarked_projects, _) = fetch_bookmarked(&user_id_str).await?;

    render_profile(
        user_info,
        profile,
        badges,
        certificates.certificates,
        tags,
        places.len(),
        lists,
        owned_projects,
        owned_events,
        joined,
        bookmarked_projects,
    )
    .await
}

/// HTMX fragment endpoint: one page of a profile section, swapped in
/// place of the section container by the pager buttons.
#[handler]
pub async fn section(
    Path(section): Path<String>,
    Query(query): Query<SectionQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;
    let user_id_str = user_info.user_id.to_string();

    let html = match section.as_str() {
        "owned-projects" => {
            let (projects, _) = fetch_owned(&user_id_str).await?;
            owned_projects_section(&projects, query.page).render_once()?
        }
        "owned-events" => {
            let (_, events) = fetch_owned(&user_id_str).await?;
            owned_events_section(&events, query.page).render_once()?
        }
        "joined" => {
            let joined = fetch_joined(&user_id_str).await?;
            joined_section(&joined, query.page).render_once()?
        }
        "saves" => {
            let (projects, _) = fetch_bookmarked(&user_id_str).await?;
            saves_section(&projects, query.page).render_once()?
        }
        other => return Err(anyhow::anyhow!("Unknown profile section: {other}")),
    };

    Ok(Html(html).into_response())
}

fn owned_projects_section(summaries: &[EntitySummary], page_num: usize) -> EntityGridSection {
    entity_grid_section(
        summaries,
        page_num,
        "/dashboard/profile/section/owned-projects",
        "prof-sec-owned-projects",
        "You do not lead any projects.",
    )
}

fn owned_events_section(summaries: &[EntitySummary], page_num: usize) -> EntityGridSection {
    entity_grid_section(
        summaries,
        page_num,
        "/dashboard/profile/section/owned-events",
        "prof-sec-owned-events",
        "You have not created any events.",
    )
}

fn joined_section(summaries: &[EntitySummary], page_num: usize) -> ProfileListSection {
    profile_list_section(
        summaries,
        page_num,
        "/dashboard/profile/section/joined",
        "prof-sec-joined",
        "You have not joined any projects via SciStarter.",
    )
}

fn saves_section(summaries: &[EntitySummary], page_num: usize) -> ProfileListSection {
    profile_list_section(
        summaries,
        page_num,
        "/dashboard/profile/section/saves",
        "prof-sec-saves",
        "You have not bookmarked any projects.",
    )
}

fn profile_list_section(
    summaries: &[EntitySummary],
    page_num: usize,
    endpoint: &str,
    target: &str,
    empty_text: &str,
) -> ProfileListSection {
    let (page_num, pages) = clamp_page(summaries.len(), page_num);
    let items = summaries
        .iter()
        .skip(page_num * SECTION_PAGE_SIZE)
        .take(SECTION_PAGE_SIZE)
        .map(EntityCardModel::from_summary)
        .collect();
    ProfileListSection {
        target: target.to_string(),
        items,
        empty_text: empty_text.to_string(),
        pager: Pager {
            page: page_num,
            pages,
            endpoint: endpoint.to_string(),
            target: target.to_string(),
        },
    }
}

/// Join interest labels for an info-table row, folding in the
/// free-text extras that accompany the tag-based entries.
fn interest_row(tags: &[String], kind: InterestKind, extra: &str) -> String {
    let mut labels = labels_for(tags, kind);
    let extra = extra.trim();
    if !extra.is_empty() {
        labels.push(extra.to_string());
    }
    labels.join(", ")
}

#[allow(clippy::too_many_arguments)]
async fn render_profile(
    user_info: Arc<BasicInfo>,
    profile: UserProfile,
    badges: Vec<UserBadge>,
    certificates: Vec<CertificateInfo>,
    tags: Vec<String>,
    places_count: usize,
    lists: Vec<EntityList>,
    owned_projects: Vec<EntitySummary>,
    owned_events: Vec<EntitySummary>,
    joined: Vec<EntitySummary>,
    bookmarked_projects: Vec<EntitySummary>,
) -> Result<Response, Error> {
    let card = ProfileCard::from_user_profile(&profile);

    let badges: Vec<BadgeView> = badges
        .iter()
        .map(|b| BadgeView {
            name: b.badge.name().to_string(),
            image: b.badge.template_url().to_string(),
        })
        .collect();

    let certificates: Vec<CertView> = certificates
        .iter()
        .map(|c| CertView {
            title: crate::certificates::title_for_mode(&c.mode),
            url: format!("/-certificate/{}", c.certificate_id),
            // created_at is an ISO timestamp; show just the date.
            issued: c.created_at.split('T').next().unwrap_or("").to_string(),
        })
        .collect();

    let interests_row = interest_row(&tags, InterestKind::Topic, "");
    let activities_row = interest_row(&tags, InterestKind::Activity, "");
    let skills_row = interest_row(&tags, InterestKind::Skill, &profile.extra_skills);
    let instruments_row = interest_row(&tags, InterestKind::Tool, &profile.extra_tools);

    let lists: Vec<ListView> = lists
        .into_iter()
        .map(|l| ListView {
            name: l.name,
            private: l.private,
        })
        .collect();

    let has_owned = !owned_projects.is_empty() || !owned_events.is_empty();

    let content = ProfileContent {
        profile,
        card,
        badges,
        certificates,
        interests_row,
        activities_row,
        skills_row,
        instruments_row,
        places_count,
        lists,
        has_owned,
        owned_projects: owned_projects_section(&owned_projects, 0),
        owned_events: owned_events_section(&owned_events, 0),
        participation: ProfileParticipation {
            joined: joined_section(&joined, 0),
            saves: saves_section(&bookmarked_projects, 0),
        },
    };

    let layout = StandardLayout::new("/dashboard/profile", content)
        .prepare(Some(user_info))
        .await?;

    ProfilePage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

struct BadgeView {
    name: String,
    image: String,
}

struct CertView {
    title: String,
    url: String,
    issued: String,
}

struct ListView {
    name: String,
    private: bool,
}

/// One page of the profile's compact entity list (small thumbnail plus
/// name per row), swapped in place by its pager.
#[derive(TemplateOnce)]
#[template(path = "dashboard/_partials/profile_list_section.stpl.html")]
pub struct ProfileListSection {
    target: String,
    items: Vec<EntityCardModel>,
    empty_text: String,
    pager: Pager,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/_partials/profile_participation.stpl.html")]
struct ProfileParticipation {
    joined: ProfileListSection,
    saves: ProfileListSection,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/profile.stpl.html")]
struct ProfilePage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<ProfileContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/profile_content.stpl.html")]
struct ProfileContent {
    profile: UserProfile,
    card: ProfileCard,
    badges: Vec<BadgeView>,
    certificates: Vec<CertView>,
    interests_row: String,
    activities_row: String,
    skills_row: String,
    instruments_row: String,
    places_count: usize,
    lists: Vec<ListView>,
    has_owned: bool,
    owned_projects: EntityGridSection,
    owned_events: EntityGridSection,
    participation: ProfileParticipation,
}
