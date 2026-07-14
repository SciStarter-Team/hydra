use std::sync::Arc;

use anyhow::Error;
use common::message::dashboard::{EntitySummaryKind, UserPlace, UserProfile};
use common::message::user::{
    BasicInfo, CertificateInfo, UserBadge, UserCertificatesRequest, UserCertificatesResponse,
};
use poem::{Response, handler, session::Session, web::RealIp};
use sailfish::TemplateOnce;

use crate::component::{Component, Head, StandardLayout};
use crate::dashboard::interests::{InterestKind, labels_for};
use crate::dashboard::{EntityCardModel, entity_summary};
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

    let owned_ids = crate::dashboard::owned_entity_ids(&user_id_str).await;
    let owned = entity_summary::lookup_batch(&owned_ids).await?;

    let joined_ids: Vec<String> =
        common::service::request("user.joins.list_by_user_id", user_id_str.clone())
            .await
            .unwrap_or_default();
    let joined = entity_summary::lookup_batch(&joined_ids).await?;

    let bookmark_ids: Vec<String> =
        common::service::request("user.bookmarks.list_by_user", user_id_str.clone())
            .await
            .unwrap_or_default();
    let bookmarked = entity_summary::lookup_batch(&bookmark_ids).await?;

    let owned_projects: Vec<EntityCardModel> = owned
        .iter()
        .filter(|s| s.kind == EntitySummaryKind::Project)
        .map(EntityCardModel::from_summary)
        .collect();
    let owned_events: Vec<EntityCardModel> = owned
        .iter()
        .filter(|s| s.kind == EntitySummaryKind::Event)
        .map(EntityCardModel::from_summary)
        .collect();
    let joined_cards: Vec<EntityCardModel> =
        joined.iter().map(EntityCardModel::from_summary).collect();
    let bookmarked_projects: Vec<EntityCardModel> = bookmarked
        .iter()
        .filter(|s| s.kind == EntitySummaryKind::Project)
        .map(EntityCardModel::from_summary)
        .collect();

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
        joined_cards,
        bookmarked_projects,
    )
    .await
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
    owned_projects: Vec<EntityCardModel>,
    owned_events: Vec<EntityCardModel>,
    joined: Vec<EntityCardModel>,
    bookmarked_projects: Vec<EntityCardModel>,
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
        owned_projects,
        owned_events,
        participation: ProfileParticipation {
            joined,
            bookmarked_projects,
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

#[derive(TemplateOnce)]
#[template(path = "dashboard/_partials/profile_participation.stpl.html")]
struct ProfileParticipation {
    joined: Vec<EntityCardModel>,
    bookmarked_projects: Vec<EntityCardModel>,
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
    owned_projects: Vec<EntityCardModel>,
    owned_events: Vec<EntityCardModel>,
    participation: ProfileParticipation,
}
