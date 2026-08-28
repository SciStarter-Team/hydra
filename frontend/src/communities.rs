use common::message::forum::{ForumInfo, GetCommunityForum};
use common::message::user::BasicInfo;
use common::message::user::{
    CommunityGetRequest, CommunityInfo, CommunityJoinRequest, CommunityLeaveRequest,
    CommunityListRequest, CommunityListResponse, CommunityMemberCheckRequest,
    CommunityMemberCheckResponse, CommunitySuccessResponse, UserCommunitiesRequest,
    UserCommunitiesResponse,
};
use poem::session::Session;
use poem::web::{Path, Query, RealIp};
use poem::{Endpoint, IntoResponse, Response, Route, get, handler, post};
use sailfish::TemplateOnce;
use std::sync::Arc;
use ulid::Ulid;

use crate::component::{Component, Forum, Head, StandardLayout};
use crate::middleware::authorization::Authenticated;
use crate::middleware::error_pages::error_response;

// Helper functions wrapping NATS calls

pub async fn list_communities(
    active_only: bool,
    featured_only: bool,
) -> Result<Vec<CommunityInfo>, anyhow::Error> {
    let response: CommunityListResponse = common::service::request(
        "user.communities.list",
        CommunityListRequest {
            active_only,
            featured_only,
        },
    )
    .await?;
    Ok(response.communities)
}

/// Look up a community by slug, or `None` if there is no such community.
///
/// Distinct from an error, which means the lookup itself failed. Collapsing
/// the two would turn a NATS outage into a cheerful "no such community".
pub async fn get_community(slug: &str) -> Result<Option<CommunityInfo>, anyhow::Error> {
    let info: Option<CommunityInfo> = common::service::request(
        "user.communities.get",
        CommunityGetRequest {
            slug: slug.to_string(),
        },
    )
    .await?;
    Ok(info)
}

pub async fn check_membership(community_id: Ulid, user_id: Ulid) -> Result<bool, anyhow::Error> {
    let response: CommunityMemberCheckResponse = common::service::request(
        "user.communities.check",
        CommunityMemberCheckRequest {
            community_id,
            user_id,
        },
    )
    .await?;
    Ok(response.is_member)
}

pub async fn user_communities(user_id: Ulid) -> Result<Vec<CommunityInfo>, anyhow::Error> {
    let response: UserCommunitiesResponse = common::service::request(
        "user.communities.user_list",
        UserCommunitiesRequest { user_id },
    )
    .await?;
    Ok(response.communities)
}

// ============================================================================
// Landing Page
// ============================================================================

#[derive(TemplateOnce)]
#[template(path = "pages/communities_landing.stpl.html")]
struct CommunitiesLandingPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<CommunitiesLandingContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "pages/communities_landing_content.stpl.html")]
struct CommunitiesLandingContent {
    user_info: Option<Arc<BasicInfo>>,
    communities: Vec<CommunityInfo>,
    joined_ids: Vec<Ulid>,
}

#[handler]
async fn landing_page(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    let communities = list_communities(true, false).await.unwrap_or_default();

    let joined_ids = if let Some(ref info) = user_info {
        user_communities(info.user_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|c| c.community_id)
            .collect()
    } else {
        vec![]
    };

    let layout = StandardLayout::new(
        "/communities",
        CommunitiesLandingContent {
            user_info: user_info.clone(),
            communities,
            joined_ids,
        },
    )
    .prepare(user_info.clone())
    .await?;

    CommunitiesLandingPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

// ============================================================================
// Detail Page
// ============================================================================

#[derive(TemplateOnce)]
#[template(path = "pages/community_detail.stpl.html")]
struct CommunityDetailPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<CommunityDetailContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "pages/community_detail_content.stpl.html")]
struct CommunityDetailContent {
    user_info: Option<Arc<BasicInfo>>,
    community: CommunityInfo,
    description_html: String,
    is_member: bool,
}

async fn ensure_community_forum(
    community: &CommunityInfo,
    user_id: Option<Ulid>,
) -> Result<ForumInfo, anyhow::Error> {
    let forum: ForumInfo = common::service::request(
        "forum.management.get_for_community",
        GetCommunityForum {
            community_id: community.community_id,
            community_slug: community.slug.clone(),
            community_name: community.name.clone(),
            user_id,
        },
    )
    .await?;
    Ok(forum)
}

/// If `forum_slug` corresponds to a community forum, returns the community's
/// slug. Otherwise returns None.
pub fn community_slug_from_forum_slug(forum_slug: &str) -> Option<&str> {
    forum_slug.strip_prefix("community-")
}

#[handler]
async fn detail_page(
    Path(slug): Path<String>,
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    let Some(community) = get_community(&slug).await? else {
        return Ok(not_found_response());
    };

    let description_html = common::quill::delta_to_html(&community.description).unwrap_or_default();

    let is_member = if let Some(ref info) = user_info {
        check_membership(community.community_id, info.user_id)
            .await
            .unwrap_or(false)
    } else {
        false
    };

    let layout = StandardLayout::new(
        &format!("/communities/{}", slug),
        CommunityDetailContent {
            user_info: user_info.clone(),
            community,
            description_html,
            is_member,
        },
    )
    .prepare(user_info.clone())
    .await?;

    CommunityDetailPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[handler]
async fn community_forum_fragment(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    let Some(community) = get_community(&slug).await? else {
        return Ok(not_found_response());
    };
    let forum_info =
        ensure_community_forum(&community, user_info.as_ref().map(|u| u.user_id)).await?;

    let prepared = Forum::new(forum_info.slug)
        .prepare(user_info.clone())
        .await?;

    Ok(poem::web::Html(prepared.render_once()?).into_response())
}

// ============================================================================
// Join / Leave Endpoints
// ============================================================================

#[derive(TemplateOnce)]
#[template(path = "pages/community_bar_fragment.stpl.html")]
struct CommunityBarFragment {
    community: CommunityInfo,
    is_member: bool,
    is_logged_in: bool,
}

#[derive(TemplateOnce)]
#[template(path = "pages/community_card_actions_fragment.stpl.html")]
struct CommunityCardActionsFragment {
    community: CommunityInfo,
    is_member: bool,
}

#[derive(serde::Deserialize)]
struct JoinLeaveQuery {
    #[serde(default)]
    context: String,
}

fn render_join_leave_response(
    community: CommunityInfo,
    is_member: bool,
    context: &str,
) -> Result<Response, anyhow::Error> {
    let html = if context == "card" {
        CommunityCardActionsFragment {
            community,
            is_member,
        }
        .render_once()?
    } else {
        CommunityBarFragment {
            community,
            is_member,
            is_logged_in: true,
        }
        .render_once()?
    };

    Ok(Response::builder().content_type("text/html").body(html))
}

/// A community named in a request path that does not exist is a missing page,
/// not a failure.
fn not_found_response() -> Response {
    error_response(poem::http::StatusCode::NOT_FOUND, "No such community.")
}

fn forbidden_response() -> Response {
    error_response(
        poem::http::StatusCode::FORBIDDEN,
        "This community is exclusive; membership is managed by administrators.",
    )
}

#[handler]
async fn join_community(
    Path(slug): Path<String>,
    Query(query): Query<JoinLeaveQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;

    let Some(community) = get_community(&slug).await? else {
        return Ok(not_found_response());
    };

    if community.exclusive {
        return Ok(forbidden_response());
    }

    common::service::request::<CommunitySuccessResponse>(
        "user.communities.join",
        CommunityJoinRequest {
            community_id: community.community_id,
            user_id: user_info.user_id,
        },
    )
    .await?;

    // Re-fetch to get updated member count
    let Some(community) = get_community(&slug).await? else {
        return Ok(not_found_response());
    };

    render_join_leave_response(community, true, &query.context)
}

#[handler]
async fn leave_community(
    Path(slug): Path<String>,
    Query(query): Query<JoinLeaveQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;

    let Some(community) = get_community(&slug).await? else {
        return Ok(not_found_response());
    };

    if community.exclusive {
        return Ok(forbidden_response());
    }

    common::service::request::<CommunitySuccessResponse>(
        "user.communities.leave",
        CommunityLeaveRequest {
            community_id: community.community_id,
            user_id: user_info.user_id,
        },
    )
    .await?;

    // Re-fetch to get updated member count
    let Some(community) = get_community(&slug).await? else {
        return Ok(not_found_response());
    };

    render_join_leave_response(community, false, &query.context)
}

// ============================================================================
// Dashboard Page
// ============================================================================

#[derive(TemplateOnce)]
#[template(path = "pages/communities_dashboard.stpl.html")]
struct CommunitiesDashboardPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<CommunitiesDashboardContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "pages/communities_dashboard_content.stpl.html")]
struct CommunitiesDashboardContent {
    communities: Vec<CommunityInfo>,
}

#[handler]
async fn dashboard_page(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;

    let communities = user_communities(user_info.user_id)
        .await
        .unwrap_or_default();

    let layout = StandardLayout::new(
        "/dashboard/communities",
        CommunitiesDashboardContent { communities },
    )
    .prepare(Some(user_info.clone()))
    .await?;

    CommunitiesDashboardPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

// ============================================================================
// Routes
// ============================================================================

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(landing_page))
        .at("/:slug", get(detail_page))
        .at("/:slug/forum", get(community_forum_fragment))
        .at("/:slug/join", post(join_community))
        .at("/:slug/leave", post(leave_community))
}

pub fn dashboard_routes() -> impl Endpoint {
    Route::new().at("/", get(dashboard_page))
}
