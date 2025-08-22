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
    pub fn new(slug: String) -> Self {
        Self {
            slug,
            page: 0,
            per_page: 20,
        }
    }

    pub fn with_pagination(slug: String, page: i64, per_page: i64) -> Self {
        Self {
            slug,
            page,
            per_page,
        }
    }
}

impl Component for Forum {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = ForumPrepared;

    async fn prepare(self, user_info: Self::Param) -> Result<Self::Prepared> {
        // Get forum by slug
        let forum: ForumInfo = request("forum.management.get_by_slug", self.slug.clone()).await?;

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
        let has_next_page = topics.len() as i64 == self.per_page;
        let has_prev_page = self.page > 0;

        Ok(ForumPrepared {
            slug: self.slug,
            forum: Some(forum),
            topics,
            user_info,
            can_create_topic,
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

pub fn forum_endpoints() -> impl poem::Endpoint {
    poem::Route::new()
        .at("/create-topic", post(create_topic_action))
        .at("/create-post", post(create_post_action))
        .at("/:slug/page", get(forum_page_htmx))
        .at("/topic/:topic_id/page", get(topic_page_htmx))
        .at("/user/:user_id/contact", get(user_personal_forum)) // Placeholder, user forums will be accessed via the profile page, which will not be part of the forum system
        .at("/user/:user_id/profile", get(user_profile_page)) // Placeholder, proper profile pages will not be part of the forum system
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
    let forum: ForumInfo = request("forum.management.get_by_slug", form.slug.clone()).await?;

    let tags: Vec<String> = form
        .tags
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let create_topic = CreateTopic {
        user_id: user_info.user_id,
        forum_id: forum.forum_id,
        title: form.title,
        content: form.content,
        category: form.category,
        tags,
    };

    let _topic: TopicInfo = request("forum.topics.create", create_topic).await?;

    // Return HTMX response to reload the forum component at page 0
    Ok(Html(format!(
        r##"<div hx-get="/-forum/{}/page?page=0" hx-target="#forum-{}" hx-trigger="load"></div>"##,
        form.slug, form.slug
    ))
    .into_response())
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
        content: form.content,
        parent_post_id,
    };

    let _post: PostInfo = request("forum.posts.create", create_post).await?;

    // Return HTMX response to reload the topic detail at the last page (where new post would be)
    Ok(Html(format!(
        r##"<div hx-get="/-forum/topic/{}/page?page=0" hx-target="#topic-{}" hx-trigger="load"></div>"##, 
        topic_id, topic_id
    )).into_response())
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
