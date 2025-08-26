use crate::timestring::TimeString;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CreateTopic {
    pub user_id: Ulid,
    pub forum_id: Ulid,
    pub title: String,
    pub content: String,
    pub category: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CreatePost {
    pub user_id: Ulid,
    pub topic_id: Ulid,
    pub content: String,
    pub parent_post_id: Option<Ulid>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TopicInfo {
    pub topic_id: Ulid,
    pub forum_id: Ulid,
    pub title: String,
    pub content: String,
    pub author_id: Ulid,
    pub author_handle: String,
    pub author_is_public: bool,
    pub created_at: TimeString,
    pub updated_at: Option<TimeString>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub post_count: i64,
    pub is_pinned: bool,
    pub is_locked: bool,
    pub is_deleted: bool,
    pub is_flagged: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PostInfo {
    pub post_id: Ulid,
    pub topic_id: Ulid,
    pub content: String,
    pub author_id: Ulid,
    pub author_handle: String,
    pub author_is_public: bool,
    pub created_at: TimeString,
    pub updated_at: Option<TimeString>,
    pub parent_post_id: Option<Ulid>,
    pub is_deleted: bool,
    pub is_flagged: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TopicListQuery {
    pub forum_id: Option<Ulid>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub search: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub sort_by: TopicSortBy,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TopicSortBy {
    CreatedDesc,
    CreatedAsc,
    UpdatedDesc,
    UpdatedAsc,
    PostCount,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PostListQuery {
    pub topic_id: Ulid,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModerationAction {
    pub user_id: Ulid,
    pub target_type: ModerationTarget,
    pub target_id: Ulid,
    pub action: ModerationActionType,
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ModerationTarget {
    Topic,
    Post,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ModerationActionType {
    Delete,
    Pin,
    Unpin,
    Lock,
    Unlock,
    Flag,
    Unflag,
}

// Forum management messages
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ForumInfo {
    pub forum_id: Ulid,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: TimeString,
    pub updated_at: Option<TimeString>,
    pub is_active: bool,
    pub display_order: i32,
    pub topic_count: i64,
    pub can_manage: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateForum {
    pub user_id: Ulid,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub display_order: Option<i32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateForum {
    pub user_id: Ulid,
    pub forum_id: Ulid,
    pub name: Option<String>,
    pub description: Option<String>,
    pub display_order: Option<i32>,
    pub is_active: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ForumListQuery {
    pub active_only: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ContentFilterResult {
    pub is_allowed: bool,
    pub flagged_words: Vec<String>,
}

// Forum moderator management messages
#[derive(Serialize, Deserialize, Debug)]
pub struct AddForumModerator {
    pub requester_id: Ulid,
    pub forum_id: Ulid,
    pub user_id: Ulid,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RemoveForumModerator {
    pub requester_id: Ulid,
    pub forum_id: Ulid,
    pub user_id: Ulid,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ForumModeratorInfo {
    pub forum_id: Ulid,
    pub user_id: Ulid,
    pub user_handle: String,
    pub created_at: TimeString,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetForumBySlug {
    pub slug: String,
    pub user_id: Option<Ulid>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetForumById {
    pub forum_id: Ulid,
    pub user_id: Option<Ulid>,
}

// Forum category management messages
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ForumCategoryInfo {
    pub category_id: Ulid,
    pub forum_id: Ulid,
    pub category_name: String,
    pub is_active: bool,
    pub created_at: TimeString,
    pub updated_at: Option<TimeString>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateForumCategory {
    pub user_id: Ulid,
    pub forum_id: Ulid,
    pub category_name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateForumCategory {
    pub user_id: Ulid,
    pub category_id: Ulid,
    pub category_name: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DeleteForumCategory {
    pub user_id: Ulid,
    pub category_id: Ulid,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetForumCategories {
    pub forum_id: Ulid,
    pub active_only: Option<bool>,
}
