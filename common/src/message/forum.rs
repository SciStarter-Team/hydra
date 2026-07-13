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

impl TopicInfo {
    pub fn paragraphs(&self) -> impl IntoIterator<Item = &str> {
        self.content.split("\n\n")
    }
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
    /// Whether this message has cleared the staff review queue. Direct
    /// messages start unapproved and are hidden from the recipient until a
    /// staff member approves them; the sender sees their own unapproved
    /// posts (badged "pending"). Non-DM forum posts are created approved.
    pub approved: bool,
}

impl PostInfo {
    pub fn paragraphs(&self) -> impl IntoIterator<Item = &str> {
        self.content.split("\n\n")
    }
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
    /// The user viewing the thread. Unapproved (pending-review) posts are
    /// only returned to their own author, so a recipient never sees a message
    /// still awaiting staff review. `None` (no specific viewer) sees only
    /// approved posts.
    pub viewer_id: Option<Ulid>,
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

#[derive(Serialize, Deserialize, Debug, Clone)]
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
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AddForumModerator {
    pub requester_id: Ulid,
    pub forum_id: Ulid,
    pub user_id: Ulid,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CreateForumCategory {
    pub user_id: Ulid,
    pub forum_id: Ulid,
    pub category_name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UpdateForumCategory {
    pub user_id: Ulid,
    pub category_id: Ulid,
    pub category_name: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DeleteForumCategory {
    pub user_id: Ulid,
    pub category_id: Ulid,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetForumCategories {
    pub forum_id: Ulid,
    pub active_only: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetCommunityForum {
    pub community_id: Ulid,
    pub community_slug: String,
    pub community_name: String,
    pub user_id: Option<Ulid>,
}

/// Personal forums (the DM inbox backing store) carry the `inbox-{id}`
/// slug convention, where `id` is the recipient's ULID — a user, project,
/// or event (all addressable the same way). They must only be written to
/// via `forum.messaging.*` endpoints, never the generic topic-creation
/// flow — otherwise any authenticated user could spam any inbox by forging
/// the slug field.
pub fn is_personal_forum_slug(slug: &str) -> bool {
    slug.starts_with("inbox-")
}

// ============================================================================
// Messaging (personal forum-based)
// ============================================================================

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SendMessage {
    pub from_user_id: Ulid,
    /// Recipient ULID — a user, project, or event. Its personal-forum
    /// inbox (`forums.forum_id == to_id`) receives the message.
    pub to_id: Ulid,
    pub subject: String,
    pub body: String,
    pub reply_ref: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SendMessageResponse {
    pub topic_id: Ulid,
    pub post_id: Ulid,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReplyToMessage {
    pub user_id: Ulid,
    pub topic_id: Ulid,
    pub body: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReplyToMessageResponse {
    pub post_id: Ulid,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InboxQuery {
    pub user_id: Ulid,
    pub include_discarded: bool,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InboxResponse {
    pub conversations: Vec<ConversationSummary>,
    /// True if there is at least one more conversation past this page,
    /// detected by peek-ahead. Reliable even when orphan filtering trims
    /// the visible page below `limit`.
    pub has_next_page: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConversationSummary {
    pub topic_id: Ulid,
    pub subject: String,
    pub other_user_id: Ulid,
    pub other_user_handle: String,
    pub last_message_at: TimeString,
    pub last_message_preview: String,
    pub post_count: i64,
    pub seen: bool,
    pub discarded: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MarkMessageRead {
    pub user_id: Ulid,
    pub topic_id: Ulid,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DiscardMessage {
    pub user_id: Ulid,
    pub topic_id: Ulid,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UnreadCountRequest {
    pub user_id: Ulid,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UnreadCountResponse {
    pub count: i64,
}

/// Query for `forum.messaging.recipients_since`. Returns the distinct set
/// of correspondents (the *other* participant ULID) that `author_id` has
/// messaged — via a new conversation or a reply — since `since`. Used to
/// enforce the "one new project/event per day" limit: the frontend keeps
/// only the project/event ULIDs from the result and counts distinct ones.
/// The response is `Vec<Ulid>`.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RecipientsSince {
    pub author_id: Ulid,
    pub since: TimeString,
}

/// One pending direct message in the staff review queue
/// (`forum.messaging.pending_list`). A message is not delivered to its
/// recipient until a staff member approves it.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReviewMessageRow {
    pub post_id: Ulid,
    pub sender: String,
    pub recipient: String,
    pub subject: String,
    pub body: String,
    pub when: String,
}

/// A staff decision on a pending message (`forum.messaging.review`): accept
/// delivers it (approved = 1); reject drops it (is_deleted = 1).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReviewMessageDecision {
    pub post_id: Ulid,
    pub accept: bool,
}
