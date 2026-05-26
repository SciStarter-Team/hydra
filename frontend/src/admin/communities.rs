use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use common::message::user::{
    CommunityCreateRequest, CommunityDeleteRequest, CommunityInfo, CommunityJoinRequest,
    CommunityLeaveRequest, CommunityListRequest, CommunityListResponse, CommunityMembersRequest,
    CommunityMembersResponse, CommunityMembership, CommunitySuccessResponse,
    CommunityUpdateRequest, UserDetails, UserDetailsListResponse, UserDetailsSearchRequest,
};
use poem::{
    Endpoint, Route, get, handler, post,
    web::{Form, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

// Templates
#[derive(TemplateOnce)]
#[template(path = "admin/communities.stpl.html")]
struct CommunitiesPage {
    communities_content: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/communities_content.stpl.html")]
struct CommunitiesContentTemplate {
    items: Vec<CommunityInfo>,
    search_query: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/modals/new_community.stpl.html")]
struct NewCommunityForm {
    error_message: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/edit_community_content.stpl.html")]
struct EditCommunityContent {
    item: CommunityInfo,
    error_message: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/community_members_content.stpl.html")]
struct CommunityMembersContent {
    community: CommunityInfo,
    members_list: CommunityMembersList,
}

#[derive(TemplateOnce, Clone)]
#[template(path = "admin/community_members_list.stpl.html")]
struct CommunityMembersList {
    community_slug: String,
    members: Vec<CommunityMembership>,
    page: u32,
    total_count: u64,
    total_pages: u32,
}

#[derive(TemplateOnce)]
#[template(path = "admin/community_members_search.stpl.html")]
struct CommunityMembersSearch {
    community_slug: String,
    users: Vec<UserDetails>,
    search: String,
}

const MEMBERS_PAGE_SIZE: u32 = 25;

// Query and form types
#[derive(Deserialize)]
struct CommunitySearchQuery {
    search: Option<String>,
}

#[derive(Deserialize)]
struct CreateCommunityForm {
    name: String,
    slug: String,
    subtitle: Option<String>,
    intro: Option<String>,
    description: Option<String>,
    image_url: Option<String>,
    partner_tag: Option<String>,
    topics: Option<String>,
    external_url: Option<String>,
    featured: Option<String>,
    active: Option<String>,
    exclusive: Option<String>,
    sort_order: Option<i32>,
}

#[derive(Deserialize)]
struct UpdateCommunityForm {
    name: String,
    slug: String,
    subtitle: Option<String>,
    intro: Option<String>,
    description: Option<String>,
    image_url: Option<String>,
    partner_tag: Option<String>,
    topics: Option<String>,
    external_url: Option<String>,
    featured: Option<String>,
    active: Option<String>,
    exclusive: Option<String>,
    sort_order: Option<i32>,
}

#[derive(Deserialize)]
struct MembersListQuery {
    page: Option<u32>,
}

#[derive(Deserialize)]
struct MemberSearchQuery {
    #[serde(default)]
    search: String,
}

// Helper functions
async fn fetch_members(
    community_id: Ulid,
    page: u32,
) -> Result<CommunityMembersResponse, anyhow::Error> {
    let response: CommunityMembersResponse = common::service::request(
        "user.communities.members",
        CommunityMembersRequest {
            community_id,
            page: Some(page),
            limit: Some(MEMBERS_PAGE_SIZE),
        },
    )
    .await?;
    Ok(response)
}

fn build_members_list(
    community_slug: &str,
    response: CommunityMembersResponse,
) -> CommunityMembersList {
    let limit = response.limit.max(1) as u64;
    let total_pages = response.total_count.div_ceil(limit) as u32;
    CommunityMembersList {
        community_slug: community_slug.to_string(),
        members: response.members,
        page: response.page,
        total_count: response.total_count,
        total_pages: total_pages.max(1),
    }
}

async fn communities_search_impl(query: &CommunitySearchQuery) -> Result<String, anyhow::Error> {
    let response: CommunityListResponse = common::service::request(
        "user.communities.list",
        CommunityListRequest {
            active_only: false,
            featured_only: false,
        },
    )
    .await?;

    let items = if let Some(ref search) = query.search {
        let search_lower = search.to_lowercase();
        response
            .communities
            .into_iter()
            .filter(|c| {
                c.name.to_lowercase().contains(&search_lower)
                    || c.slug.to_lowercase().contains(&search_lower)
                    || c.partner_tag.to_lowercase().contains(&search_lower)
            })
            .collect()
    } else {
        response.communities
    };

    let template = CommunitiesContentTemplate {
        items,
        search_query: query.search.clone(),
    };

    Ok(template.render_once()?)
}

// Handlers
#[handler]
async fn communities_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let search_results = communities_search_impl(&CommunitySearchQuery { search: None }).await?;

    let content = format!(r#"<div id="communities-content">{}</div>"#, search_results);

    let page = CommunitiesPage {
        communities_content: content,
    };

    let prepared = AdminLayout::new("Communities", page)
        .with_sidebar_active("communities")
        .with_breadcrumb("Communities", None)
        .with_quill()
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn communities_search(
    Query(query): Query<CommunitySearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = communities_search_impl(&query).await?;
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(content))
}

#[handler]
async fn communities_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let form = NewCommunityForm {
        error_message: None,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}

#[handler]
async fn communities_create(
    Form(form): Form<CreateCommunityForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let topics: Vec<String> = form
        .topics
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    match common::service::request::<CommunitySuccessResponse>(
        "user.communities.create",
        CommunityCreateRequest {
            name: form.name,
            slug: form.slug,
            subtitle: form.subtitle.unwrap_or_default(),
            intro: form.intro.unwrap_or_default(),
            description: form.description.unwrap_or_default(),
            image_url: form.image_url.unwrap_or_default(),
            partner_tag: form.partner_tag.unwrap_or_default(),
            topics,
            external_url: form.external_url.unwrap_or_default(),
            active: form.active.is_some(),
            featured: form.featured.is_some(),
            exclusive: form.exclusive.is_some(),
            sort_order: form.sort_order.unwrap_or(0),
        },
    )
    .await
    {
        Ok(_) => {
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "community-create",
                    user_info.user_id,
                    None::<ulid::Ulid>,
                    None::<String>,
                )
                .await
                .ok();
            }

            let content = communities_search_impl(&CommunitySearchQuery { search: None }).await?;
            let html = format!(
                r#"<div id="communities-content" hx-swap-oob="innerHTML">{}</div>"#,
                content
            );

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(html))
        }
        Err(e) => {
            let form = NewCommunityForm {
                error_message: Some(e.to_string()),
            };

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(form.render_once()?))
        }
    }
}

#[handler]
async fn communities_edit(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    // Find community by slug (we use slug as the URL param for readability)
    let item: CommunityInfo = common::service::request(
        "user.communities.get",
        common::message::user::CommunityGetRequest { slug: id_str },
    )
    .await?;

    let content = EditCommunityContent {
        item,
        error_message: None,
    };

    let prepared = AdminLayout::new("Edit Community", content)
        .with_breadcrumb("Communities", Some("/-admin/communities"))
        .with_breadcrumb("Edit", None)
        .with_sidebar_active("communities")
        .with_quill()
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn communities_update(
    Path(id_str): Path<String>,
    Form(form): Form<UpdateCommunityForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let community: CommunityInfo = common::service::request(
        "user.communities.get",
        common::message::user::CommunityGetRequest { slug: id_str },
    )
    .await?;

    let topics: Vec<String> = form
        .topics
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    match common::service::request::<CommunitySuccessResponse>(
        "user.communities.update",
        CommunityUpdateRequest {
            community_id: community.community_id,
            name: form.name,
            slug: form.slug,
            subtitle: form.subtitle.unwrap_or_default(),
            intro: form.intro.unwrap_or_default(),
            description: form.description.unwrap_or_default(),
            image_url: form.image_url.unwrap_or_default(),
            partner_tag: form.partner_tag.unwrap_or_default(),
            topics,
            external_url: form.external_url.unwrap_or_default(),
            active: form.active.is_some(),
            featured: form.featured.is_some(),
            exclusive: form.exclusive.is_some(),
            sort_order: form.sort_order.unwrap_or(0),
        },
    )
    .await
    {
        Ok(_) => {
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "community-update",
                    user_info.user_id,
                    community.community_id,
                    None::<String>,
                )
                .await
                .ok();
            }

            Ok(poem::Response::builder()
                .status(poem::http::StatusCode::SEE_OTHER)
                .header("Location", "/-admin/communities")
                .finish())
        }
        Err(e) => {
            let item: CommunityInfo = common::service::request(
                "user.communities.get",
                common::message::user::CommunityGetRequest {
                    slug: community.slug,
                },
            )
            .await?;

            let content = EditCommunityContent {
                item,
                error_message: Some(e.to_string()),
            };

            let layout = AdminLayout::new("Edit Community", content)
                .with_breadcrumb("Communities", Some("/-admin/communities"))
                .with_breadcrumb("Edit", None)
                .with_sidebar_active("communities")
                .with_quill()
                .prepare(user_info)
                .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(layout.render_once()?))
        }
    }
}

#[handler]
async fn communities_delete(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let community: CommunityInfo = common::service::request(
        "user.communities.get",
        common::message::user::CommunityGetRequest { slug: id_str },
    )
    .await?;

    common::service::request::<CommunitySuccessResponse>(
        "user.communities.delete",
        CommunityDeleteRequest {
            community_id: community.community_id,
        },
    )
    .await?;

    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            "community-delete",
            user_info.user_id,
            community.community_id,
            Some(format!("name={}", community.name)),
        )
        .await
        .ok();
    }

    let content = communities_search_impl(&CommunitySearchQuery { search: None }).await?;
    let html = format!(
        r#"<div id="communities-content" hx-swap-oob="innerHTML">{}</div>"#,
        content
    );

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

// ============================================================================
// Member Management
// ============================================================================

async fn render_members_list(
    community_slug: &str,
    community_id: Ulid,
    page: u32,
) -> Result<String, anyhow::Error> {
    let response = fetch_members(community_id, page).await?;
    let list = build_members_list(community_slug, response);
    Ok(list.render_once()?)
}

#[handler]
async fn community_members_page(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let community: CommunityInfo = common::service::request(
        "user.communities.get",
        common::message::user::CommunityGetRequest { slug: slug.clone() },
    )
    .await?;

    let response = fetch_members(community.community_id, 1).await?;
    let members_list = build_members_list(&community.slug, response);

    let content = CommunityMembersContent {
        community: community.clone(),
        members_list,
    };

    let prepared = AdminLayout::new("Community Members", content)
        .with_breadcrumb("Communities", Some("/-admin/communities"))
        .with_breadcrumb(
            &community.name,
            Some(&format!("/-admin/communities/{}", community.slug)),
        )
        .with_breadcrumb("Members", None)
        .with_sidebar_active("communities")
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn community_members_list_fragment(
    Path(slug): Path<String>,
    Query(query): Query<MembersListQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let community: CommunityInfo = common::service::request(
        "user.communities.get",
        common::message::user::CommunityGetRequest { slug: slug.clone() },
    )
    .await?;

    let html = render_members_list(
        &community.slug,
        community.community_id,
        query.page.unwrap_or(1),
    )
    .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn community_members_search(
    Path(slug): Path<String>,
    Query(query): Query<MemberSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let community: CommunityInfo = common::service::request(
        "user.communities.get",
        common::message::user::CommunityGetRequest { slug: slug.clone() },
    )
    .await?;

    let search = query.search.trim().to_string();

    let users = if search.is_empty() {
        Vec::new()
    } else {
        let response: UserDetailsListResponse = common::service::request(
            "user.admin.user_search",
            UserDetailsSearchRequest {
                search: Some(search.clone()),
                role: None,
                email_status: None,
                page: Some(1),
                limit: Some(20),
            },
        )
        .await?;
        response.users
    };

    let template = CommunityMembersSearch {
        community_slug: community.slug,
        users,
        search,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(template.render_once()?))
}

#[handler]
async fn community_member_add(
    Path((slug, user_id_str)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let community: CommunityInfo = common::service::request(
        "user.communities.get",
        common::message::user::CommunityGetRequest { slug: slug.clone() },
    )
    .await?;

    let user_id: Ulid = user_id_str.parse()?;

    common::service::request::<CommunitySuccessResponse>(
        "user.communities.join",
        CommunityJoinRequest {
            community_id: community.community_id,
            user_id,
        },
    )
    .await?;

    if let Some(admin) = &user_info {
        common::service::log_activity(
            None,
            "community-admin-add-member",
            admin.user_id,
            community.community_id,
            Some(format!("member={}", user_id)),
        )
        .await
        .ok();
    }

    let html = render_members_list(&community.slug, community.community_id, 1).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn community_member_remove(
    Path((slug, user_id_str)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let community: CommunityInfo = common::service::request(
        "user.communities.get",
        common::message::user::CommunityGetRequest { slug: slug.clone() },
    )
    .await?;

    let user_id: Ulid = user_id_str.parse()?;

    common::service::request::<CommunitySuccessResponse>(
        "user.communities.leave",
        CommunityLeaveRequest {
            community_id: community.community_id,
            user_id,
        },
    )
    .await?;

    if let Some(admin) = &user_info {
        common::service::log_activity(
            None,
            "community-admin-remove-member",
            admin.user_id,
            community.community_id,
            Some(format!("member={}", user_id)),
        )
        .await
        .ok();
    }

    let html = render_members_list(&community.slug, community.community_id, 1).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

// Routes export
pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(communities_list).post(communities_create))
        .at("/search", get(communities_search))
        .at("/new", get(communities_new))
        .at("/:id/members", get(community_members_page))
        .at("/:id/members/list", get(community_members_list_fragment))
        .at("/:id/members/search", get(community_members_search))
        .at(
            "/:id/members/:user_id",
            post(community_member_add).delete(community_member_remove),
        )
        .at(
            "/:id",
            get(communities_edit)
                .post(communities_update)
                .delete(communities_delete),
        )
}
