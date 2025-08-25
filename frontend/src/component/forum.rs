use std::sync::Arc;

use anyhow::Result;
use common::{
    message::{forum::*, user::BasicInfo},
    service::request,
};
use poem::{
    IntoResponse, Response, get, handler, post,
    session::Session,
    web::{Form, Html, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::{component::Component, middleware::authorization::Authenticated};

pub struct Forum {
    pub slug: String,
    pub page: i64,
    pub per_page: i64,
}

impl Default for Forum {
    fn default() -> Self {
        Self {
            slug: String::new(),
            page: 0,
            per_page: 20,
        }
    }
}

impl Forum {
    pub fn new(slug: impl Into<String>) -> Self {
        Self {
            slug: slug.into(),
            page: 0,
            per_page: 20,
        }
    }

    pub fn with_pagination(slug: impl Into<String>, page: i64, per_page: i64) -> Self {
        Self {
            slug: slug.into(),
            page,
            per_page,
        }
    }
}

impl Component for Forum {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = ForumPrepared;

    async fn prepare(self, user_info: Self::Param) -> Result<Self::Prepared> {
        // Get forum by slug with user_id for permission checking
        let get_forum_request = GetForumBySlug {
            slug: self.slug.clone(),
            user_id: user_info.as_ref().map(|u| u.user_id),
        };
        let forum: ForumInfo = request("forum.management.get_by_slug", get_forum_request).await?;

        // Get topics for this forum with pagination
        let offset = self.page * self.per_page;
        let topic_query = TopicListQuery {
            forum_id: Some(forum.forum_id),
            category: None,
            tags: vec![],
            search: None,
            limit: Some(self.per_page),
            offset: Some(offset),
            sort_by: TopicSortBy::UpdatedDesc,
        };

        let topics: Vec<TopicInfo> = request("forum.topics.list", topic_query).await?;

        let can_create_topic = user_info.is_some();
        let can_manage_forum = forum.can_manage;
        let has_next_page = topics.len() as i64 == self.per_page;
        let has_prev_page = self.page > 0;

        Ok(ForumPrepared {
            slug: self.slug,
            forum: Some(forum),
            topics,
            user_info,
            can_create_topic,
            can_manage_forum,
            page: self.page,
            per_page: self.per_page,
            has_next_page,
            has_prev_page,
        })
    }
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "forum/forum.stpl.html")]
pub struct ForumPrepared {
    pub slug: String,
    pub forum: Option<ForumInfo>,
    pub topics: Vec<TopicInfo>,
    pub user_info: Option<Arc<BasicInfo>>,
    pub can_create_topic: bool,
    pub can_manage_forum: bool,
    pub page: i64,
    pub per_page: i64,
    pub has_next_page: bool,
    pub has_prev_page: bool,
}

pub struct TopicDetail {
    pub topic: TopicInfo,
    pub page: i64,
    pub per_page: i64,
}

impl TopicDetail {
    pub fn new(topic: TopicInfo) -> Self {
        Self {
            topic,
            page: 0,
            per_page: 20,
        }
    }

    pub fn with_pagination(topic: TopicInfo, page: i64, per_page: i64) -> Self {
        Self {
            topic,
            page,
            per_page,
        }
    }
}

impl Component for TopicDetail {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = TopicDetailPrepared;

    async fn prepare(self, user_info: Self::Param) -> Result<Self::Prepared> {
        // Get posts for this topic with pagination
        let offset = self.page * self.per_page;
        let post_query = PostListQuery {
            topic_id: self.topic.topic_id,
            limit: Some(self.per_page),
            offset: Some(offset),
        };

        let posts: Vec<PostInfo> = request("forum.posts.list", post_query).await?;
        let can_reply = user_info.is_some();
        let has_next_page = posts.len() as i64 == self.per_page;
        let has_prev_page = self.page > 0;

        Ok(TopicDetailPrepared {
            topic: self.topic,
            posts,
            user_info,
            can_reply,
            page: self.page,
            per_page: self.per_page,
            has_next_page,
            has_prev_page,
        })
    }
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "forum/topic_detail.stpl.html")]
pub struct TopicDetailPrepared {
    pub topic: TopicInfo,
    pub posts: Vec<PostInfo>,
    pub user_info: Option<Arc<BasicInfo>>,
    pub can_reply: bool,
    pub page: i64,
    pub per_page: i64,
    pub has_next_page: bool,
    pub has_prev_page: bool,
}

#[derive(Deserialize)]
struct CreateTopicForm {
    slug: String,
    title: String,
    content: String,
    category: Option<String>,
    tags: String, // Comma-separated tags
}

#[derive(Deserialize)]
struct CreatePostForm {
    topic_id: String,
    content: String,
    parent_post_id: Option<String>,
}

#[derive(Deserialize)]
struct PaginationQuery {
    page: Option<i64>,
    per_page: Option<i64>,
}

#[derive(Deserialize)]
struct UpdateForumForm {
    name: String,
    description: String,
    is_active: Option<String>, // HTML checkbox sends value or nothing
}

#[derive(Deserialize)]
struct AddModeratorForm {
    user_id: String,
    user_handle: String, // For display purposes
}

#[derive(Deserialize)]
struct SuggestQuery {
    title: String,
}

#[derive(Deserialize)]
struct UserSearchQuery {
    q: String,
}

pub fn forum_endpoints() -> impl poem::Endpoint {
    poem::Route::new()
        .at("/topic/new", post(create_topic_action))
        .at("/topic/:topic_id/page", get(topic_page_htmx))
        .at("/post/new", post(create_post_action))
        .at("/user/:user_id/contact", get(user_personal_forum)) // Placeholder, user forums will be accessed via the profile page, which will not be part of the forum system
        .at("/user/:user_id/profile", get(user_profile_page)) // Placeholder, proper profile pages will not be part of the forum system
        .at("/:slug/page", get(forum_page_htmx))
        .at("/:slug/suggest", get(suggest_similar_topics_action))
        .at("/:slug/update", post(update_forum_action))
        .at("/:slug/moderators", get(list_forum_moderators_action))
        .at("/:slug/users/search", get(search_users_action))
        .at("/:slug/moderators/add", post(add_forum_moderator_action))
        .at(
            "/:slug/moderators/:user_id/remove",
            post(remove_forum_moderator_action),
        )
}

#[handler]
async fn forum_page_htmx(
    Path(slug): Path<String>,
    Query(query): Query<PaginationQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let page = query.page.unwrap_or(0);
    let per_page = query.per_page.unwrap_or(20);

    let forum = Forum::with_pagination(slug, page, per_page);
    let prepared = forum.prepare(user_info).await?;

    Ok(Html(prepared.render_once()?).into_response())
}

#[handler]
async fn topic_page_htmx(
    Path(topic_id): Path<String>,
    Query(query): Query<PaginationQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let page = query.page.unwrap_or(0);
    let per_page = query.per_page.unwrap_or(20);

    // Get the topic first
    let topic: TopicInfo = request("forum.topics.get", topic_id).await?;

    let topic_detail = TopicDetail::with_pagination(topic, page, per_page);
    let prepared = topic_detail.prepare(user_info).await?;

    Ok(Html(prepared.render_once()?).into_response())
}

#[handler]
async fn create_topic_action(
    Form(form): Form<CreateTopicForm>,
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;

    // Get forum by slug to get forum_id
    let get_forum_request = GetForumBySlug {
        slug: form.slug.clone(),
        user_id: Some(user_info.user_id),
    };
    let forum: ForumInfo = request("forum.management.get_by_slug", get_forum_request).await?;

    let tags: Vec<String> = form
        .tags
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let create_topic = CreateTopic {
        user_id: user_info.user_id,
        forum_id: forum.forum_id,
        title: form.title.clone(),
        content: form.content.clone(),
        category: form.category.clone(),
        tags,
    };

    match request::<TopicInfo>("forum.topics.create", create_topic).await {
        Ok(_topic) => {
            // Success - reload the forum component at page 0
            Ok(Html(format!(
                r##"<div hx-get="/-forum/{}/page?page=0" hx-target="#forum-{}" hx-trigger="load"></div>"##,
                form.slug, form.slug
            )).into_response())
        }
        Err(error) => {
            // Show error message for any other issues (content filter, etc.)
            let error_msg = error.to_string();
            Ok(Html(format!(
                r##"<div class="error-message">
                    <h4>Error creating topic</h4>
                    <p>{}</p>
                    <button hx-on:click="document.getElementById('create-topic-form').style.display='block'; this.remove();" class="btn-secondary">Try Again</button>
                </div>"##,
                error_msg.replace('"', "&quot;")
            )).into_response())
        }
    }
}

#[handler]
async fn create_post_action(
    Form(form): Form<CreatePostForm>,
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;

    let topic_id = form.topic_id.parse::<Ulid>()?;

    let parent_post_id = if let Some(pid) = form.parent_post_id {
        Some(pid.parse::<Ulid>()?)
    } else {
        None
    };

    let create_post = CreatePost {
        user_id: user_info.user_id,
        topic_id,
        content: form.content.clone(),
        parent_post_id,
    };

    match request::<PostInfo>("forum.posts.create", create_post).await {
        Ok(_post) => {
            // Success - reload the topic detail at the last page (where new post would be)
            Ok(Html(format!(
                r##"<div hx-get="/-forum/topic/{}/page?page=0" hx-target="#topic-{}" hx-trigger="load"></div>"##, 
                topic_id, topic_id
            )).into_response())
        }
        Err(error) => {
            // Show error message for any issues (content filter, etc.)
            let error_msg = error.to_string();
            Ok(Html(format!(
                r##"<div class="error-message">
                    <h4>Error creating post</h4>
                    <p>{}</p>
                    <button hx-on:click="document.querySelector('form[hx-target=\"#topic-{}\"] textarea').focus(); this.remove();" class="btn-secondary">Try Again</button>
                </div>"##,
                error_msg.replace('"', "&quot;"), topic_id
            )).into_response())
        }
    }
}

#[handler]
async fn user_personal_forum(
    Path(user_id): Path<String>,
    Authenticated(viewer_info): Authenticated,
) -> Result<Response> {
    let user_id_ulid = user_id.parse::<Ulid>()?;

    // Get the target user's info to check privacy
    let target_user: BasicInfo = request("user.basic_info.by_user_id", user_id.clone()).await?;

    // If user is private, return error or empty response
    if target_user.is_private() {
        return Ok(Html("<p>This user's profile is private.</p>".to_string()).into_response());
    }

    // Get or create their personal forum
    let personal_forum: ForumInfo = request("forum.management.get_personal", user_id_ulid).await?;

    // Use the personal forum slug to load forum component
    let forum = Forum::new(personal_forum.slug);
    let prepared = forum.prepare(viewer_info).await?;

    Ok(Html(prepared.render_once()?).into_response())
}

#[handler]
async fn user_profile_page(
    Path(user_id): Path<String>,
    Authenticated(viewer_info): Authenticated,
) -> Result<Response> {
    let target_user: BasicInfo = request("user.basic_info.by_user_id", user_id.clone()).await?;

    // If user is private, show minimal info
    if target_user.is_private() {
        return Ok(Html(format!(
            r#"<div class="user-profile">
                <h2>Private Profile</h2>
                <p>This user has chosen to keep their profile private.</p>
            </div>"#
        ))
        .into_response());
    }

    // Show public profile with contact link
    Ok(Html(format!(
        r#"<div class="user-profile">
            <h2>{}'s Profile</h2>
            <div class="profile-actions">
                <a href="/-forum/user/{}/contact" class="btn contact-btn">Contact {}</a>
            </div>
        </div>"#,
        target_user.handle, user_id, target_user.handle
    ))
    .into_response())
}

#[handler]
async fn update_forum_action(
    Path(slug): Path<String>,
    Form(form): Form<UpdateForumForm>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;

    // Get forum by slug to get forum_id
    let get_forum_request = GetForumBySlug {
        slug: slug.clone(),
        user_id: Some(user_info.user_id),
    };
    let forum: ForumInfo = request("forum.management.get_by_slug", get_forum_request).await?;

    // Check if user can manage this forum (should be true if we got here)
    if !forum.can_manage {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let update_forum = UpdateForum {
        user_id: user_info.user_id,
        forum_id: forum.forum_id,
        name: Some(form.name),
        description: Some(form.description),
        display_order: None, // Keep current display_order
        is_active: Some(form.is_active.is_some()),
    };

    let _updated_forum: ForumInfo = request("forum.management.update", update_forum).await?;

    // Return HTMX response to reload the forum component
    Ok(Html(format!(
        r##"<div hx-get="/-forum/{}/page?page=0" hx-target="#forum-{}" hx-trigger="load"></div>"##,
        slug, slug
    ))
    .into_response())
}

async fn list_forum_moderators_impl(slug: &str, user_info: &BasicInfo) -> Result<Response> {
    // Get forum by slug to get forum_id
    let get_forum_request = GetForumBySlug {
        slug: slug.to_string(),
        user_id: Some(user_info.user_id),
    };
    let forum: ForumInfo = request("forum.management.get_by_slug", get_forum_request).await?;

    // Check if user can manage this forum
    if !forum.can_manage {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let moderators: Vec<ForumModeratorInfo> = request(
        "forum.moderation.list_moderators",
        (user_info.user_id, forum.forum_id),
    )
    .await?;

    let mut html = String::from("<div class='moderator-list'><h6>Current Moderators:</h6>");
    if moderators.is_empty() {
        html.push_str("<p>No moderators assigned to this forum.</p>");
    } else {
        html.push_str("<ul>");
        for moderator in moderators {
            html.push_str(&format!(
                "<li>{} <button hx-post='/-forum/{}/moderators/{}/remove' hx-target='#moderator-list' class='btn-danger btn-small'>Remove</button></li>",
                moderator.user_handle, slug, moderator.user_id
            ));
        }
        html.push_str("</ul>");
    }
    html.push_str("</div>");

    Ok(Html(html).into_response())
}

#[handler]
async fn list_forum_moderators_action(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;
    list_forum_moderators_impl(&slug, &user_info).await
}

#[handler]
async fn add_forum_moderator_action(
    Path(slug): Path<String>,
    Form(form): Form<AddModeratorForm>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;

    // Get forum by slug to get forum_id
    let get_forum_request = GetForumBySlug {
        slug: slug.clone(),
        user_id: Some(user_info.user_id),
    };
    let forum: ForumInfo = request("forum.management.get_by_slug", get_forum_request).await?;

    // Check if user can manage this forum
    if !forum.can_manage {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    // Parse user_id from form (already validated by autocomplete selection)
    let target_user_id = form.user_id.parse::<Ulid>()?;

    let add_moderator = AddForumModerator {
        requester_id: user_info.user_id,
        forum_id: forum.forum_id,
        user_id: target_user_id,
    };

    request::<()>("forum.moderation.add_moderator", add_moderator).await?;

    // Return updated moderator list
    list_forum_moderators_impl(&slug, &user_info).await
}

#[handler]
async fn remove_forum_moderator_action(
    Path((slug, user_id_str)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;
    let target_user_id = user_id_str.parse::<Ulid>()?;

    // Get forum by slug to get forum_id
    let get_forum_request = GetForumBySlug {
        slug: slug.clone(),
        user_id: Some(user_info.user_id),
    };
    let forum: ForumInfo = request("forum.management.get_by_slug", get_forum_request).await?;

    // Check if user can manage this forum
    if !forum.can_manage {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let remove_moderator = RemoveForumModerator {
        requester_id: user_info.user_id,
        forum_id: forum.forum_id,
        user_id: target_user_id,
    };

    request::<()>("forum.moderation.remove_moderator", remove_moderator).await?;

    // Return updated moderator list
    list_forum_moderators_impl(&slug, &user_info).await
}

#[handler]
async fn suggest_similar_topics_action(
    Path(slug): Path<String>,
    Query(query): Query<SuggestQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    // Only show suggestions if user can create topics (is logged in)
    if user_info.is_none() {
        return Ok(Html("".to_string()).into_response());
    }
    
    // Only show suggestions if title is long enough to be meaningful
    if query.title.trim().len() < 3 {
        return Ok(Html("".to_string()).into_response());
    }

    // Get similar topics suggestions using slug directly
    let similar_topics: Result<Vec<TopicInfo>, _> = request(
        "forum.topics.suggest_similar",
        (slug.clone(), query.title.trim().to_string())
    ).await;

    match similar_topics {
        Ok(topics) if !topics.is_empty() => {
            let mut html = String::from(r#"<div class="similar-topics-suggestions">"#);
            html.push_str("<h5>Similar topics found:</h5>");
            html.push_str("<div class=\"suggested-topics\">");
            
            for topic in topics.iter().take(3) { // Show max 3 suggestions
                html.push_str(&format!(
                    r##"<div class="suggested-topic" 
                            hx-get="/-forum/topic/{}/page?page=0" 
                            hx-target="#topic-{}" 
                            hx-trigger="click"
                            style="cursor: pointer;">
                        <div class="topic-title">{}</div>
                        <small>by {} • {} posts</small>
                    </div>"##,
                    topic.topic_id, topic.topic_id, topic.title, topic.author_handle, topic.post_count
                ));
            }
            
            html.push_str("</div></div>");
            Ok(Html(html).into_response())
        }
        _ => Ok(Html("".to_string()).into_response())
    }
}

#[handler]
async fn search_users_action(
    Path(_slug): Path<String>,
    Query(query): Query<UserSearchQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    // Only allow authenticated users to search for users
    if user_info.is_none() {
        return Ok(Html("".to_string()).into_response());
    }

    // Only search if query is long enough to be meaningful
    if query.q.trim().len() < 2 {
        return Ok(Html("".to_string()).into_response());
    }

    // Search for users by handle using the user service
    let search_results: Result<Vec<BasicInfo>, _> = request(
        "user.search.by_handle", 
        query.q.trim().to_string()
    ).await;

    match search_results {
        Ok(users) if !users.is_empty() => {
            let mut html = String::from(r#"<div class="user-search-results">"#);
            
            for user in users {
                html.push_str(&format!(
                    r##"<div class="user-search-result" 
                            data-user-id="{}"
                            data-user-handle="{}"
                            hx-on:click="selectUser('{}', '{}')"
                            style="cursor: pointer;">
                        <div class="user-info">
                            <strong>{}</strong>
                            <small>ID: {}</small>
                        </div>
                    </div>"##,
                    user.user_id, user.handle, user.user_id, user.handle,
                    user.display_handle(), 
                    user.user_id.to_string() // Show full ULID for proper disambiguation
                ));
            }
            
            html.push_str("</div>");
            Ok(Html(html).into_response())
        }
        _ => Ok(Html("".to_string()).into_response())
    }
}
