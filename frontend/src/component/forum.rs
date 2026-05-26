use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::Result;
use common::{
    message::{forum::*, user::BasicInfo},
    service::{request, request_with_retry},
};
use poem::{
    EndpointExt, IntoResponse, Response, get, handler, post,
    web::{Form, Html, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::{
    component::Component,
    local_time::{TimeFormat, local_time},
    middleware::{
        authorization::Authenticated,
        rate_limit::{RateLimitConfig, RateLimitMiddleware},
    },
};

pub fn forum_routes() -> impl poem::Endpoint {
    // Per-IP write limits. Generous for legitimate use but tight enough to
    // make slug-bypass / mass-posting attacks impractical even if a future
    // gap reopens them.
    let topic_limit = RateLimitConfig::new(5, Duration::from_secs(300))
        .with_error_message("Too many new topics. Please wait a few minutes before posting again.");
    let post_limit = RateLimitConfig::new(20, Duration::from_secs(300))
        .with_error_message("Too many replies. Please wait a few minutes before posting again.");

    poem::Route::new()
        .at(
            "/topic/new",
            post(create_topic_action).with(RateLimitMiddleware::new(topic_limit)),
        )
        .at("/topic/:topic_id/page", get(topic_page_htmx))
        .at("/topic/:topic_id/card", get(topic_card_htmx))
        .at(
            "/post/new",
            post(create_post_action).with(RateLimitMiddleware::new(post_limit)),
        )
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
        .at("/:slug/categories", get(list_forum_categories_action))
        .at("/:slug/categories/add", post(create_forum_category_action))
        .at(
            "/:slug/categories/:category_id/update",
            post(update_forum_category_action),
        )
        .at(
            "/:slug/categories/:category_id/delete",
            post(delete_forum_category_action),
        )
        .at("/moderate", post(moderate_content_action))
}

#[derive(Debug)]
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

    #[tracing::instrument]
    async fn prepare(self, user_info: Self::Param) -> Result<Self::Prepared> {
        // Get forum by slug with user_id for permission checking
        let get_forum_request = GetForumBySlug {
            slug: self.slug.clone(),
            user_id: user_info.as_ref().map(|u| u.user_id),
        };
        let forum: ForumInfo = request("forum.management.get_by_slug", get_forum_request).await?;

        // Parallelize topics, categories, and membership lookup
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

        let categories_query = GetForumCategories {
            forum_id: forum.forum_id,
            active_only: Some(true),
        };

        let community_slug =
            crate::communities::community_slug_from_forum_slug(&forum.slug).map(|s| s.to_string());

        let membership_fut =
            resolve_community_membership(community_slug.as_deref(), user_info.as_ref());

        let (topics_result, categories_result, membership) = tokio::join!(
            request::<Vec<TopicInfo>>("forum.topics.list", topic_query),
            request::<Vec<ForumCategoryInfo>>("forum.categories.list", categories_query),
            membership_fut,
        );

        let topics = topics_result?;
        let categories = categories_result.unwrap_or_else(|_| vec![]);

        let is_personal_forum = common::message::forum::is_personal_forum_slug(&forum.slug);
        let can_create_topic = if is_personal_forum {
            // Personal forums are DM inboxes; new topics there come from
            // forum.messaging.send, not the generic composer.
            false
        } else {
            match (user_info.is_some(), &membership) {
                (false, _) => false,
                (true, CommunityMembership::None) => true,
                (true, CommunityMembership::Member) => true,
                (true, CommunityMembership::NotMember) => false,
            }
        };
        let can_manage_forum = forum.can_manage;
        let can_manage_moderators = user_info.as_ref().map_or(false, |user| user.is_staff);
        let has_next_page = topics.len() as i64 == self.per_page;
        let has_prev_page = self.page > 0;

        Ok(ForumPrepared {
            slug: self.slug,
            forum: Some(forum),
            topics,
            categories,
            user_info,
            can_create_topic,
            can_manage_forum,
            can_manage_moderators,
            community_slug,
            page: self.page,
            per_page: self.per_page,
            has_next_page,
            has_prev_page,
        })
    }
}

/// Membership status of the viewing user relative to the forum's gating
/// community (if any).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommunityMembership {
    /// Forum is not community-gated, or there is no logged-in user to evaluate.
    None,
    /// Forum is community-gated and the user belongs to it.
    Member,
    /// Forum is community-gated and the user does not belong to it.
    NotMember,
}

async fn resolve_community_membership(
    community_slug: Option<&str>,
    user_info: Option<&Arc<BasicInfo>>,
) -> CommunityMembership {
    let Some(community_slug) = community_slug else {
        return CommunityMembership::None;
    };
    let Some(user) = user_info else {
        return CommunityMembership::NotMember;
    };
    let Ok(community) = crate::communities::get_community(community_slug).await else {
        return CommunityMembership::NotMember;
    };
    if crate::communities::check_membership(community.community_id, user.user_id)
        .await
        .unwrap_or(false)
    {
        CommunityMembership::Member
    } else {
        CommunityMembership::NotMember
    }
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "forum/forum.stpl.html")]
pub struct ForumPrepared {
    pub slug: String,
    pub forum: Option<ForumInfo>,
    pub topics: Vec<TopicInfo>,
    pub categories: Vec<ForumCategoryInfo>,
    pub user_info: Option<Arc<BasicInfo>>,
    pub can_create_topic: bool,
    pub can_manage_forum: bool,
    pub can_manage_moderators: bool,
    pub community_slug: Option<String>,
    pub page: i64,
    pub per_page: i64,
    pub has_next_page: bool,
    pub has_prev_page: bool,
}

#[derive(Debug)]
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

    #[tracing::instrument]
    async fn prepare(self, user_info: Self::Param) -> Result<Self::Prepared> {
        // Get posts for this topic with pagination
        let offset = self.page * self.per_page;
        let post_query = PostListQuery {
            topic_id: self.topic.topic_id,
            limit: Some(self.per_page),
            offset: Some(offset),
        };

        let forum_request = GetForumById {
            forum_id: self.topic.forum_id,
            user_id: user_info.as_ref().map(|u| u.user_id),
        };

        let (posts_result, forum_result) = tokio::join!(
            request::<Vec<PostInfo>>("forum.posts.list", post_query),
            request::<ForumInfo>("forum.management.get_by_id", forum_request),
        );

        let posts = posts_result?;
        let has_next_page = posts.len() as i64 == self.per_page;
        let has_prev_page = self.page > 0;

        let (community_slug, can_moderate) = match forum_result {
            Ok(forum) => (
                crate::communities::community_slug_from_forum_slug(&forum.slug)
                    .map(|s| s.to_string()),
                forum.can_manage,
            ),
            Err(_) => (None, false),
        };

        let membership =
            resolve_community_membership(community_slug.as_deref(), user_info.as_ref()).await;

        let can_reply = match (user_info.is_some(), membership) {
            (false, _) => false,
            (true, CommunityMembership::NotMember) => false,
            (true, _) => true,
        };

        let threaded_posts = ThreadedPost::build_thread(posts);

        Ok(TopicDetailPrepared {
            topic: self.topic,
            threaded_posts,
            user_info,
            can_reply,
            can_moderate,
            community_slug,
            page: self.page,
            per_page: self.per_page,
            has_next_page,
            has_prev_page,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ThreadedPost {
    pub post: PostInfo,
    pub replies: Vec<ThreadedPost>,
    pub depth: usize,
}

impl ThreadedPost {
    #[tracing::instrument]
    fn build_thread(posts: Vec<PostInfo>) -> Vec<ThreadedPost> {
        let mut children: HashMap<Ulid, Vec<PostInfo>> = HashMap::new();
        let mut root_posts = Vec::new();

        // Index all posts and group by parent
        for post in posts {
            if let Some(parent_id) = post.parent_post_id {
                children.entry(parent_id).or_default().push(post);
            } else {
                root_posts.push(post);
            }
        }

        // Build threaded structure starting from root posts
        #[tracing::instrument]
        fn build_threaded(
            post: PostInfo,
            children: &HashMap<Ulid, Vec<PostInfo>>,
            depth: usize,
        ) -> ThreadedPost {
            let post_id = post.post_id;
            let replies = if let Some(child_posts) = children.get(&post_id) {
                child_posts
                    .iter()
                    .map(|child| build_threaded(child.clone(), children, depth + 1))
                    .collect()
            } else {
                Vec::new()
            };

            ThreadedPost {
                post,
                replies,
                depth,
            }
        }

        root_posts
            .into_iter()
            .map(|post| build_threaded(post, &children, 0))
            .collect()
    }
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "forum/topic_detail.stpl.html")]
pub struct TopicDetailPrepared {
    pub topic: TopicInfo,
    pub threaded_posts: Vec<ThreadedPost>,
    pub user_info: Option<Arc<BasicInfo>>,
    pub can_reply: bool,
    pub can_moderate: bool,
    pub community_slug: Option<String>,
    pub page: i64,
    pub per_page: i64,
    pub has_next_page: bool,
    pub has_prev_page: bool,
}

// Template structs for HTML fragments
#[derive(TemplateOnce)]
#[template(path = "forum/threaded_post.stpl.html")]
pub struct ThreadedPostDisplay<'a> {
    pub post: &'a ThreadedPost,
    pub topic_id: Ulid,
    pub can_reply: bool,
    pub can_moderate: bool,
}

#[derive(TemplateOnce)]
#[template(path = "forum/topic_card.stpl.html")]
pub struct TopicCard<'a> {
    pub topic: &'a TopicInfo,
}

#[derive(TemplateOnce)]
#[template(path = "forum/htmx_reload.stpl.html")]
pub struct HtmxReload<'a> {
    pub target_url: &'a str,
    pub target_selector: &'a str,
}

#[derive(TemplateOnce)]
#[template(path = "forum/error_message.stpl.html")]
pub struct ErrorMessage<'a> {
    pub title: &'a str,
    pub message: &'a str,
    pub button_text: Option<&'a str>,
    pub button_action: Option<&'a str>,
}

#[derive(TemplateOnce)]
#[template(path = "forum/moderator_list.stpl.html")]
pub struct ModeratorList<'a> {
    pub slug: &'a str,
    pub moderators: &'a [ForumModeratorInfo],
}

#[derive(TemplateOnce)]
#[template(path = "forum/user_search_results.stpl.html")]
pub struct UserSearchResults<'a> {
    pub users: &'a [BasicInfo],
}

#[derive(TemplateOnce)]
#[template(path = "forum/category_list.stpl.html")]
pub struct CategoryList<'a> {
    pub slug: &'a str,
    pub categories: &'a [ForumCategoryInfo],
}

#[derive(TemplateOnce)]
#[template(path = "forum/category_dropdown.stpl.html")]
pub struct CategoryDropdown<'a> {
    pub categories: &'a [ForumCategoryInfo],
    pub oob: bool,
}

#[derive(TemplateOnce)]
#[template(path = "forum/similar_topics.stpl.html")]
pub struct SimilarTopics<'a> {
    pub topics: &'a [TopicInfo],
}

#[derive(TemplateOnce)]
#[template(path = "forum/user_profile.stpl.html")]
pub struct UserProfile<'a> {
    pub user_id: &'a str,
    pub user_handle: &'a str,
    pub is_private: bool,
}

#[derive(TemplateOnce)]
#[template(path = "forum/moderation_actions.stpl.html")]
pub struct ModerationActions<'a> {
    pub target_type: &'a str,
    pub target_id: &'a str,
    pub topic_id: &'a str,
    pub is_pinned: bool,
    pub is_locked: bool,
    pub is_flagged: bool,
}

#[derive(Debug, Deserialize)]
struct CreateTopicForm {
    slug: String,
    title: String,
    content: String,
    category: Option<String>,
    tags: String, // Comma-separated tags
}

#[derive(Debug, Deserialize)]
struct CreatePostForm {
    topic_id: String,
    content: String,
    parent_post_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PaginationQuery {
    page: Option<i64>,
    per_page: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UpdateForumForm {
    name: String,
    description: String,
    is_active: Option<String>, // HTML checkbox sends value or nothing
}

#[derive(Debug, Deserialize)]
struct AddModeratorForm {
    user_id: String,
}

#[derive(Debug, Deserialize)]
struct SuggestQuery {
    title: String,
}

#[derive(Debug, Deserialize)]
struct UserSearchQuery {
    q: String,
}

#[derive(Debug, Deserialize)]
struct CreateCategoryForm {
    slug: String,
    category_name: String,
}

#[derive(Debug, Deserialize)]
struct UpdateCategoryForm {
    category_name: Option<String>,
    is_active: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModerationForm {
    target_type: String,
    target_id: String,
    action: String,
    reason: Option<String>,
}

#[handler]
#[tracing::instrument]
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
#[tracing::instrument]
async fn topic_card_htmx(Path(topic_id): Path<String>) -> Result<Response> {
    let topic: TopicInfo = request("forum.topics.get", topic_id).await?;
    let card = TopicCard { topic: &topic };
    Ok(Html(card.render_once()?).into_response())
}

#[handler]
#[tracing::instrument]
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
#[tracing::instrument]
async fn create_topic_action(
    Form(form): Form<CreateTopicForm>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;

    // Personal forums are the DM inbox backing store; the only legitimate
    // write path is forum.messaging.send via /dashboard/messages/send.
    if common::message::forum::is_personal_forum_slug(&form.slug) {
        let error_message = ErrorMessage {
            title: "Not allowed",
            message: "Personal forums can only be written to via direct messages.",
            button_text: None,
            button_action: None,
        };
        return Ok(Html(error_message.render_once()?).into_response());
    }

    // For community forums, require membership
    if let Some(community_slug) = crate::communities::community_slug_from_forum_slug(&form.slug) {
        let community = crate::communities::get_community(community_slug).await?;
        if !crate::communities::check_membership(community.community_id, user_info.user_id)
            .await
            .unwrap_or(false)
        {
            let error_message = ErrorMessage {
                title: "Join required",
                message: "Join this community to post a discussion.",
                button_text: None,
                button_action: None,
            };
            return Ok(Html(error_message.render_once()?).into_response());
        }
    }

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

    match request_with_retry::<TopicInfo>("forum.topics.create", create_topic).await {
        Ok(_topic) => {
            // Success - directly render the updated forum component
            let forum_component = Forum::with_pagination(form.slug, 0, 20);
            let prepared = forum_component.prepare(Some(user_info)).await?;
            Ok(Html(prepared.render_once()?).into_response())
        }
        Err(error) => {
            // Show error message for any other issues (content filter, etc.)
            let message = error.to_string().replace('"', "&quot;");
            let button_action =
                "document.getElementById('create-topic-form').style.display='block'";
            let error_message = ErrorMessage {
                title: "Error creating topic",
                message: &message,
                button_text: Some("Try Again"),
                button_action: Some(button_action),
            };
            Ok(Html(error_message.render_once()?).into_response())
        }
    }
}

#[handler]
#[tracing::instrument]
async fn create_post_action(
    Form(form): Form<CreatePostForm>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;

    let topic_id = form.topic_id.parse::<Ulid>()?;

    // For community forums, require membership
    let topic_for_gate: TopicInfo = request("forum.topics.get", topic_id.to_string()).await?;
    let forum_info: ForumInfo = request(
        "forum.management.get_by_id",
        GetForumById {
            forum_id: topic_for_gate.forum_id,
            user_id: Some(user_info.user_id),
        },
    )
    .await?;
    if let Some(community_slug) =
        crate::communities::community_slug_from_forum_slug(&forum_info.slug)
    {
        let community = crate::communities::get_community(community_slug).await?;
        if !crate::communities::check_membership(community.community_id, user_info.user_id)
            .await
            .unwrap_or(false)
        {
            let error_message = ErrorMessage {
                title: "Join required",
                message: "Join this community to reply.",
                button_text: None,
                button_action: None,
            };
            return Ok(Html(error_message.render_once()?).into_response());
        }
    }

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

    match request_with_retry::<PostInfo>("forum.posts.create", create_post).await {
        Ok(_post) => {
            // Success - return the updated topic content
            let topic: TopicInfo = request("forum.topics.get", topic_id.to_string()).await?;
            let topic_detail = TopicDetail::with_pagination(topic, 0, 20);
            let prepared = topic_detail.prepare(Some(user_info)).await?;
            Ok(Html(prepared.render_once()?).into_response())
        }
        Err(error) => {
            // Show error message for any issues (content filter, etc.)
            let error_str = error.to_string();
            let message = if error_str.contains("Content contains prohibited words") {
                // Extract just the prohibited words part and format nicely
                if let Some(words_start) = error_str.find('[') {
                    if let Some(words_end) = error_str.find(']') {
                        let words_part = &error_str[words_start..=words_end];
                        format!("Your post contains prohibited words: {}", words_part)
                    } else {
                        "Your post contains prohibited words".to_string()
                    }
                } else {
                    "Your post contains prohibited words".to_string()
                }
            } else {
                error_str
            };

            // Use htmx utilities to find and restore the reply form
            let form_id = if let Some(parent_id) = parent_post_id {
                format!("reply-form-{}", parent_id)
            } else {
                "reply-form-topic".to_string()
            };

            let button_action = format!(
                "let form = htmx.find('#{}'); \
                 if (form) {{ \
                     form.style.display = 'block'; \
                     let textarea = htmx.find(form, 'textarea'); \
                     if (textarea) {{ \
                         textarea.focus(); \
                         textarea.value = '{}'; \
                     }} \
                 }}",
                form_id,
                form.content
                    .replace('\'', "\\'")
                    .replace('\n', "\\n")
                    .replace('\r', "")
            );

            let error_message = ErrorMessage {
                title: "Error creating post",
                message: &message,
                button_text: Some("Try Again"),
                button_action: Some(&button_action),
            };

            // Use HTMX headers to show error after the form instead of replacing content
            let mut response = Html(error_message.render_once()?).into_response();
            response
                .headers_mut()
                .insert("HX-Reswap", "afterend".parse().unwrap());
            response.headers_mut().insert(
                "HX-Retarget",
                if let Some(_) = parent_post_id {
                    format!("#reply-form-{}", parent_post_id.unwrap())
                        .parse()
                        .unwrap()
                } else {
                    "#reply-form-topic".parse().unwrap()
                },
            );
            Ok(response)
        }
    }
}

#[handler]
#[tracing::instrument]
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
#[tracing::instrument]
async fn user_profile_page(
    Path(user_id): Path<String>,
    Authenticated(_viewer_info): Authenticated,
) -> Result<Response> {
    let target_user: BasicInfo = request("user.basic_info.by_user_id", user_id.clone()).await?;

    let profile = UserProfile {
        user_id: &user_id,
        user_handle: &target_user.handle,
        is_private: target_user.is_private(),
    };
    Ok(Html(profile.render_once()?).into_response())
}

#[handler]
#[tracing::instrument]
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

    let _updated_forum: ForumInfo =
        request_with_retry("forum.management.update", update_forum).await?;

    // Return HTMX response to reload the forum component
    let target_url = format!("/-forum/{}/page?page=0", slug);
    let target_selector = format!("#forum-{}", slug);
    let reload = HtmxReload {
        target_url: &target_url,
        target_selector: &target_selector,
    };
    Ok(Html(reload.render_once()?).into_response())
}

#[tracing::instrument]
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

    let moderator_list = ModeratorList {
        slug,
        moderators: &moderators,
    };
    Ok(Html(moderator_list.render_once()?).into_response())
}

#[handler]
#[tracing::instrument]
async fn list_forum_moderators_action(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;
    list_forum_moderators_impl(&slug, &user_info).await
}

#[handler]
#[tracing::instrument]
async fn add_forum_moderator_action(
    Path(slug): Path<String>,
    Form(form): Form<AddModeratorForm>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;

    // Validate that a user was selected
    if form.user_id.trim().is_empty() {
        let error_message = ErrorMessage {
            title: "No User Selected",
            message: "Please select a user from the search results before adding them as a moderator.",
            button_text: None,
            button_action: None,
        };

        let mut response = Html(error_message.render_once()?).into_response();
        response.headers_mut().insert(
            "HX-Retarget",
            format!("#forum-{} .moderator-errors", slug)
                .parse()
                .unwrap(),
        );
        return Ok(response);
    }

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

    // Parse user_id from form - now we know it's not empty
    let target_user_id = match form.user_id.parse::<Ulid>() {
        Ok(id) => id,
        Err(_) => {
            let error_message = ErrorMessage {
                title: "Invalid User Selection",
                message: "Please select a valid user from the search results.",
                button_text: None,
                button_action: None,
            };

            let mut response = Html(error_message.render_once()?).into_response();
            response.headers_mut().insert(
                "HX-Retarget",
                format!("#forum-{} .moderator-errors", slug)
                    .parse()
                    .unwrap(),
            );
            return Ok(response);
        }
    };

    let add_moderator = AddForumModerator {
        requester_id: user_info.user_id,
        forum_id: forum.forum_id,
        user_id: target_user_id,
    };

    match request_with_retry::<()>("forum.moderation.add_moderator", add_moderator).await {
        Ok(_) => {
            // Return updated moderator list
            list_forum_moderators_impl(&slug, &user_info).await
        }
        Err(error) => {
            // Create user-friendly error message based on actual backend responses
            let error_str = error.to_string();
            let message = match error_str.as_str() {
                msg if msg.contains("Target user not found") => {
                    "User not found. Please check the username and try again."
                }
                msg if msg
                    .contains("Staff members cannot be added as forum-specific moderators") =>
                {
                    "Staff members and administrators cannot be added as moderators."
                }
                msg if msg
                    .contains("Permission denied: only staff can manage forum moderators") =>
                {
                    "You don't have permission to add moderators to this forum."
                }
                // Database constraint error for duplicate moderator
                msg if msg.contains("UNIQUE constraint failed") || msg.contains("duplicate") => {
                    "This user is already a moderator of this forum."
                }
                _ => {
                    tracing::warn!("Unhandled add moderator error: {}", error_str);
                    "Failed to add moderator. Please try again."
                }
            };

            let error_message = ErrorMessage {
                title: "Cannot Add Moderator",
                message,
                button_text: None,
                button_action: None,
            };

            // Use HX-Retarget to target the specific forum's moderator error container
            let mut response = Html(error_message.render_once()?).into_response();
            response.headers_mut().insert(
                "HX-Retarget",
                format!("#forum-{} .moderator-errors", slug)
                    .parse()
                    .unwrap(),
            );
            Ok(response)
        }
    }
}

#[handler]
#[tracing::instrument]
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

    request_with_retry::<()>("forum.moderation.remove_moderator", remove_moderator).await?;

    // Return updated moderator list
    list_forum_moderators_impl(&slug, &user_info).await
}

#[handler]
#[tracing::instrument]
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
        (slug.clone(), query.title.trim().to_string()),
    )
    .await;

    match similar_topics {
        Ok(topics) if !topics.is_empty() => {
            let similar = SimilarTopics { topics: &topics };
            Ok(Html(similar.render_once()?).into_response())
        }
        _ => Ok(Html("".to_string()).into_response()),
    }
}

#[handler]
#[tracing::instrument]
async fn search_users_action(
    Path(slug): Path<String>,
    Query(query): Query<UserSearchQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    // Restrict to forum moderators: this search powers the moderator-add UI
    // in forum.stpl.html. Without a per-forum gate any logged-in user could
    // enumerate the public user base by iterating short handle prefixes.
    let Some(user) = user_info else {
        return Ok(Html("".to_string()).into_response());
    };
    let forum: ForumInfo = request(
        "forum.management.get_by_slug",
        GetForumBySlug {
            slug: slug.clone(),
            user_id: Some(user.user_id),
        },
    )
    .await?;
    if !forum.can_manage {
        return Ok(Html("".to_string()).into_response());
    }

    // Only search if query is long enough to be meaningful
    if query.q.trim().len() < 2 {
        return Ok(Html("".to_string()).into_response());
    }

    // Search for users by handle using the user service
    let search_results: Result<Vec<BasicInfo>, _> =
        request("user.search.by_handle", query.q.trim().to_string()).await;

    match search_results {
        Ok(users) if !users.is_empty() => {
            let search_results = UserSearchResults { users: &users };
            Ok(Html(search_results.render_once()?).into_response())
        }
        _ => Ok(Html("".to_string()).into_response()),
    }
}

#[tracing::instrument]
async fn generate_category_list_html(slug: &str, user_info: &BasicInfo) -> Result<String> {
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

    // Get categories for this forum
    let categories_query = GetForumCategories {
        forum_id: forum.forum_id,
        active_only: Some(false), // Show all categories for management
    };
    let categories: Vec<ForumCategoryInfo> = request("forum.categories.list", categories_query)
        .await
        .unwrap_or_else(|_| vec![]);

    let category_list = CategoryList {
        slug,
        categories: &categories,
    };
    Ok(category_list.render_once()?)
}

#[tracing::instrument]
async fn generate_category_dropdown_oob(slug: &str, user_info: &BasicInfo) -> Result<String> {
    // Get forum by slug to get forum_id
    let get_forum_request = GetForumBySlug {
        slug: slug.to_string(),
        user_id: Some(user_info.user_id),
    };
    let forum: ForumInfo = request("forum.management.get_by_slug", get_forum_request).await?;

    // Get active categories for this forum (only active ones for dropdowns)
    let categories_query = GetForumCategories {
        forum_id: forum.forum_id,
        active_only: Some(true),
    };
    let categories: Vec<ForumCategoryInfo> = request("forum.categories.list", categories_query)
        .await
        .unwrap_or_else(|_| vec![]);

    let dropdown = CategoryDropdown {
        categories: &categories,
        oob: true,
    };
    Ok(dropdown.render_once()?)
}

#[handler]
#[tracing::instrument]
async fn list_forum_categories_action(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;
    let html = generate_category_list_html(&slug, &user_info).await?;
    Ok(Html(html).into_response())
}

#[handler]
#[tracing::instrument]
async fn create_forum_category_action(
    Form(form): Form<CreateCategoryForm>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;

    // Get forum by slug to get forum_id
    let get_forum_request = GetForumBySlug {
        slug: form.slug.clone(),
        user_id: Some(user_info.user_id),
    };
    let forum: ForumInfo = request("forum.management.get_by_slug", get_forum_request).await?;

    // Check if user can manage this forum
    if !forum.can_manage {
        return Err(anyhow::anyhow!("Permission denied"));
    }

    let create_category = CreateForumCategory {
        user_id: user_info.user_id,
        forum_id: forum.forum_id,
        category_name: form.category_name.trim().to_string(),
    };

    match request_with_retry::<ForumCategoryInfo>("forum.categories.create", create_category).await
    {
        Ok(_) => {
            // Return the updated category list AND update all dropdowns
            let list_html = generate_category_list_html(&form.slug, &user_info).await?;
            let dropdown_oob = generate_category_dropdown_oob(&form.slug, &user_info).await?;
            Ok(Html(format!("{}{}", list_html, dropdown_oob)).into_response())
        }
        Err(error) => {
            // Create user-friendly error message based on actual backend responses
            let error_str = error.to_string();
            let message = match error_str.as_str() {
                msg if msg.contains("Category already exists for this forum") => {
                    "A category with this name already exists."
                }
                msg if msg.contains("Permission denied") => {
                    "You don't have permission to create categories in this forum."
                }
                _ => {
                    tracing::warn!("Unhandled create category error: {}", error_str);
                    "Failed to create category. Please try again."
                }
            };

            let error_message = ErrorMessage {
                title: "Cannot Create Category",
                message,
                button_text: None,
                button_action: None,
            };

            // Use HX-Retarget to target the specific forum's category error container
            let mut response = Html(error_message.render_once()?).into_response();
            response.headers_mut().insert(
                "HX-Retarget",
                format!("#forum-{} .category-errors", form.slug)
                    .parse()
                    .unwrap(),
            );
            Ok(response)
        }
    }
}

#[handler]
#[tracing::instrument]
async fn update_forum_category_action(
    Path((slug, category_id_str)): Path<(String, String)>,
    Form(form): Form<UpdateCategoryForm>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;
    let category_id = category_id_str.parse::<Ulid>()?;

    let update_category = UpdateForumCategory {
        user_id: user_info.user_id,
        category_id,
        category_name: form
            .category_name
            .as_ref()
            .map(|name| name.trim().to_string()),
        is_active: form.is_active.as_ref().and_then(|v| match v.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }),
    };

    match request_with_retry::<ForumCategoryInfo>("forum.categories.update", update_category).await
    {
        Ok(_) => {
            // Return the updated category list AND update all dropdowns
            let list_html = generate_category_list_html(&slug, &user_info).await?;
            let dropdown_oob = generate_category_dropdown_oob(&slug, &user_info).await?;
            Ok(Html(format!("{}{}", list_html, dropdown_oob)).into_response())
        }
        Err(error) => {
            // Create user-friendly error message
            let message = match error.to_string().as_str() {
                msg if msg.contains("not found") => {
                    "Category not found. It may have been deleted by another user."
                }
                msg if msg.contains("already exists") => {
                    "A category with this name already exists."
                }
                msg if msg.contains("invalid name") => {
                    "Category name is invalid. Please use only letters, numbers, and spaces."
                }
                msg if msg.contains("permission denied") => {
                    "You don't have permission to modify categories in this forum."
                }
                _ => "Failed to update category. Please try again.",
            };

            let error_message = ErrorMessage {
                title: "Cannot Update Category",
                message,
                button_text: None,
                button_action: None,
            };

            // Use HX-Retarget to target the specific forum's error container
            let mut response = Html(error_message.render_once()?).into_response();
            response.headers_mut().insert(
                "HX-Retarget",
                format!("#forum-{} .category-errors", slug).parse().unwrap(),
            );
            Ok(response)
        }
    }
}

#[handler]
#[tracing::instrument]
async fn delete_forum_category_action(
    Path((slug, category_id_str)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;
    let category_id = category_id_str.parse::<Ulid>()?;

    let delete_category = DeleteForumCategory {
        user_id: user_info.user_id,
        category_id,
    };

    match request_with_retry::<()>("forum.categories.delete", delete_category).await {
        Ok(_) => {
            // Return the updated category list AND update all dropdowns
            let list_html = generate_category_list_html(&slug, &user_info).await?;
            let dropdown_oob = generate_category_dropdown_oob(&slug, &user_info).await?;
            Ok(Html(format!("{}{}", list_html, dropdown_oob)).into_response())
        }
        Err(error) => {
            // Create user-friendly error message
            let message = match error.to_string().as_str() {
                msg if msg.contains("not found") => {
                    "Category not found. It may have been deleted by another user."
                }
                msg if msg.contains("has topics") => {
                    "Cannot delete category because it contains topics. Move or delete topics first."
                }
                msg if msg.contains("permission denied") => {
                    "You don't have permission to delete categories in this forum."
                }
                _ => "Failed to delete category. Please try again.",
            };

            let error_message = ErrorMessage {
                title: "Cannot Delete Category",
                message,
                button_text: None,
                button_action: None,
            };

            // Use HX-Retarget to target the specific forum's error container
            let mut response = Html(error_message.render_once()?).into_response();
            response.headers_mut().insert(
                "HX-Retarget",
                format!("#forum-{} .category-errors", slug).parse().unwrap(),
            );
            Ok(response)
        }
    }
}

#[handler]
#[tracing::instrument]
async fn moderate_content_action(
    Form(form): Form<ModerationForm>,
    Authenticated(user_info): Authenticated,
) -> Result<Response> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Must be logged in"))?;

    let target_id = form.target_id.parse::<Ulid>()?;

    // Convert form action to ModerationActionType
    let action = match form.action.as_str() {
        "delete" => ModerationActionType::Delete,
        "pin" => ModerationActionType::Pin,
        "unpin" => ModerationActionType::Unpin,
        "lock" => ModerationActionType::Lock,
        "unlock" => ModerationActionType::Unlock,
        "flag" => ModerationActionType::Flag,
        "unflag" => ModerationActionType::Unflag,
        _ => return Err(anyhow::anyhow!("Invalid moderation action")),
    };

    // Convert form target_type to ModerationTarget
    let target_type = match form.target_type.as_str() {
        "topic" => ModerationTarget::Topic,
        "post" => ModerationTarget::Post,
        _ => return Err(anyhow::anyhow!("Invalid moderation target")),
    };

    let moderation_action = ModerationAction {
        user_id: user_info.user_id,
        target_type,
        target_id,
        action,
        reason: form.reason,
    };

    match request_with_retry::<()>("forum.moderation.action", moderation_action).await {
        Ok(_) => {
            // Success - return the updated topic content directly
            let topic_id = match form.target_type.as_str() {
                "topic" => target_id,
                "post" => {
                    // For posts, get the topic they belong to
                    let post: PostInfo = request("forum.posts.get", target_id.to_string()).await?;
                    post.topic_id
                }
                _ => return Err(anyhow::anyhow!("Invalid target type")),
            };

            let topic: TopicInfo = request("forum.topics.get", topic_id.to_string()).await?;
            let topic_detail = TopicDetail::with_pagination(topic, 0, 20);
            let prepared = topic_detail.prepare(Some(user_info)).await?;
            Ok(Html(prepared.render_once()?).into_response())
        }
        Err(error) => {
            let message = error.to_string().replace('"', "&quot;");
            let error_message = ErrorMessage {
                title: "Moderation Error",
                message: &message,
                button_text: Some("OK"),
                button_action: None,
            };
            Ok(Html(error_message.render_once()?).into_response())
        }
    }
}
