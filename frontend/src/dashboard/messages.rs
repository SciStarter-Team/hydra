use std::collections::HashSet;

use anyhow::{Error, anyhow};
use common::message::forum::{
    ConversationSummary, DiscardMessage, ForumInfo, GetForumById, InboxQuery, InboxResponse,
    MarkMessageRead, PostInfo, PostListQuery, RecipientsSince, ReplyToMessage,
    ReplyToMessageResponse, SendMessage, SendMessageResponse, TopicInfo, is_personal_forum_slug,
};
use common::message::user::BasicInfo;
use common::service::{request, send as nats_send};
use common::timestring::TimeString;
use poem::{
    IntoResponse, Response, handler,
    session::Session,
    web::{Form, Html, Path, Query, RealIp, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::component::{Component, Head, StandardLayout};
use crate::dashboard::entity_summary;
use crate::local_time::{TimeFormat, local_time};
use crate::middleware::authorization::Authenticated;

/// Maximum number of distinct projects/events a member may message per day.
const DAILY_ENTITY_LIMIT: usize = 10;

/// Enforce the daily distinct-project/event message limit.
///
/// Returns `Ok(None)` when the user may message `target_entity_id`, or
/// `Ok(Some(message))` when they have already messaged `DAILY_ENTITY_LIMIT`
/// *different* projects/events today. Entities already contacted today can
/// always be messaged again (a conversation continues), and messaging a
/// fellow user never reaches this helper. Mirrors orrery's rule
/// (projects/events are limited; users are not).
async fn daily_entity_limit_block(
    user_id: Ulid,
    target_entity_id: Ulid,
) -> Result<Option<String>, Error> {
    let recipients: Vec<Ulid> = request(
        "forum.messaging.recipients_since",
        RecipientsSince {
            author_id: user_id,
            since: TimeString::start_of_utc_day(),
        },
    )
    .await?;

    let recipient_ids: Vec<String> = recipients.iter().map(|id| id.to_string()).collect();
    let messaged_today: HashSet<Ulid> = entity_summary::lookup_batch(&recipient_ids)
        .await?
        .into_iter()
        .map(|summary| summary.id)
        .collect();

    if messaged_today.contains(&target_entity_id) || messaged_today.len() < DAILY_ENTITY_LIMIT {
        Ok(None)
    } else {
        Ok(Some(
            "You can only message 10 different projects per day. You've reached \
             today's limit — you can keep replying to projects you've already \
             contacted, but please wait until tomorrow to contact new ones."
                .to_string(),
        ))
    }
}

/// Render a message-limit rejection as an HTMX-swappable fragment.
fn limit_error_fragment(message: &str) -> Response {
    Html(format!("<p class=\"text-sm text-red-600\">{message}</p>")).into_response()
}

/// The confirmation shown after a message is accepted for delivery. Every
/// message is held for staff review before it reaches its recipient, so we
/// tell the sender that — and warn that spam gets their account flagged.
pub fn pending_review_fragment() -> Response {
    Html(
        "<div class=\"ss-card !p-4 text-sm\">\
           <p class=\"font-semibold text-primary-dark\">Your message is pending review.</p>\
           <p class=\"mt-1 text-gray-600\">If we detect spam, your account will be flagged and potentially blocked.</p>\
         </div>"
            .to_string(),
    )
    .into_response()
}

/// Confirmation shown when a message is delivered immediately with no review —
/// used for trusted, entity-authored sends (a manager replying or broadcasting
/// as the project). `message` is the already-escaped/plain confirmation text.
pub fn sent_fragment(message: &str) -> Response {
    Html(format!(
        "<div class=\"ss-card !p-4 text-sm\">\
           <p class=\"font-semibold text-primary-dark\">{message}</p>\
         </div>"
    ))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct InboxPageQuery {
    #[serde(default)]
    pub page: Option<i64>,
}

const INBOX_PAGE_SIZE: i64 = 25;

#[handler]
pub async fn page(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
    Query(q): Query<InboxPageQuery>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;

    let page = q.page.unwrap_or(0).max(0);
    let offset = page * INBOX_PAGE_SIZE;

    let inbox: InboxResponse = request(
        "forum.messaging.inbox",
        InboxQuery {
            user_id: user_info.user_id,
            include_discarded: false,
            limit: Some(INBOX_PAGE_SIZE),
            offset: Some(offset),
        },
    )
    .await?;

    // Classify the correspondents so conversations with a project/event
    // render as an entity (name, no "@") rather than assuming a member handle.
    let other_ids: Vec<String> = inbox
        .conversations
        .iter()
        .map(|c| c.other_user_id.to_string())
        .collect();
    let entity_ids: HashSet<Ulid> = entity_summary::lookup_batch(&other_ids)
        .await?
        .into_iter()
        .map(|summary| summary.id)
        .collect();

    let conversations: Vec<ConversationModel> = inbox
        .conversations
        .into_iter()
        .map(|conv| {
            let is_entity = entity_ids.contains(&conv.other_user_id);
            ConversationModel {
                is_entity,
                ..ConversationModel::from(conv)
            }
        })
        .collect();

    let content = InboxContent {
        conversations,
        page,
        has_prev_page: page > 0,
        has_next_page: inbox.has_next_page,
    };

    let layout = StandardLayout::new("/dashboard/messages", content)
        .prepare(Some(user_info))
        .await?;

    InboxPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(Debug, Deserialize)]
pub struct ComposeQuery {
    pub to: String,
}

#[handler]
pub async fn compose(
    Authenticated(user_info): Authenticated,
    Query(q): Query<ComposeQuery>,
) -> Result<Response, Error> {
    let _ = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let target_id: Ulid =
        q.to.parse()
            .map_err(|err| anyhow!("Invalid recipient: {}", err))?;

    let target: BasicInfo = request("user.basic_info.by_user_id", target_id.to_string()).await?;

    let html = ComposeFragment {
        target_user_id: target_id,
        target_handle: target.handle,
    }
    .render_once()?;

    Ok(Html(html).into_response())
}

#[derive(Debug, Deserialize)]
pub struct SendForm {
    pub to: String,
    pub subject: String,
    pub body: String,
}

#[handler]
pub async fn send(
    Authenticated(user_info): Authenticated,
    Form(form): Form<SendForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let to_id: Ulid = form
        .to
        .parse()
        .map_err(|err| anyhow!("Invalid recipient: {}", err))?;

    if form.subject.trim().is_empty() || form.body.trim().is_empty() {
        return Err(anyhow!("Subject and body are required"));
    }

    // Messaging a project/event is subject to the one-per-day limit;
    // messaging a fellow user is not. Classify the recipient locally.
    if let Some(entity) = entity_summary::lookup_batch(&[to_id.to_string()])
        .await?
        .into_iter()
        .next()
    {
        if let Some(message) = daily_entity_limit_block(user_info.user_id, entity.id).await? {
            return Ok(limit_error_fragment(&message));
        }
    }

    let _: SendMessageResponse = request(
        "forum.messaging.send",
        SendMessage {
            from_user_id: user_info.user_id,
            to_id,
            subject: form.subject,
            body: form.body,
            reply_ref: None,
            // A member's message is held for staff review.
            pre_approved: false,
        },
    )
    .await?;

    // The message is held for staff review, not delivered yet — tell the sender.
    Ok(pending_review_fragment())
}

#[derive(Debug, Deserialize)]
pub struct ThreadQuery {
    #[serde(default)]
    pub from: Option<i64>,
}

#[handler]
pub async fn thread(
    Authenticated(user_info): Authenticated,
    Path(topic_id): Path<String>,
    Query(q): Query<ThreadQuery>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let topic_ulid: Ulid = topic_id
        .parse()
        .map_err(|err| anyhow!("Invalid topic id: {}", err))?;

    let topic: TopicInfo = request("forum.topics.get", topic_id.clone()).await?;

    // Reject topics that aren't in a personal forum: this view assumes
    // forum_id is the recipient's user_id (used below to identify the
    // "other" participant). A community-forum topic that the user happens
    // to have authored would otherwise be reachable here.
    let forum: ForumInfo = request(
        "forum.management.get_by_id",
        GetForumById {
            forum_id: topic.forum_id,
            user_id: Some(user_info.user_id),
        },
    )
    .await?;
    if !is_personal_forum_slug(&forum.slug) {
        return Err(anyhow!("Not a direct message"));
    }

    if topic.author_id != user_info.user_id && topic.forum_id != user_info.user_id {
        return Err(anyhow!("You are not a participant in this conversation"));
    }

    let posts: Vec<PostInfo> = request(
        "forum.posts.list",
        PostListQuery {
            topic_id: topic_ulid,
            limit: Some(200),
            offset: Some(0),
            // The viewer sees approved posts plus their own pending ones.
            viewer_id: Some(user_info.user_id),
        },
    )
    .await?;

    nats_send(
        "forum.messaging.read",
        MarkMessageRead {
            user_id: user_info.user_id,
            topic_id: topic_ulid,
        },
    )
    .await?;

    let post_models: Vec<PostModel> = posts
        .into_iter()
        .map(|p| PostModel {
            is_self: p.author_id == user_info.user_id,
            author_id: p.author_id,
            author_handle: p.author_handle,
            paragraphs: p.content.split("\n\n").map(|s| s.to_string()).collect(),
            created_at: p.created_at.to_string(),
            approved: p.approved,
        })
        .collect();

    let other_user_id = if topic.author_id == user_info.user_id {
        topic.forum_id
    } else {
        topic.author_id
    };

    // The other participant is a user (profile link) or a project/event
    // (entity-page link). Resolve which so the thread links correctly — a
    // conversation with a project must not link to a nonexistent profile.
    let (other_user_handle, other_link, other_is_entity) =
        match entity_summary::lookup_batch(&[other_user_id.to_string()])
            .await?
            .into_iter()
            .next()
        {
            Some(entity) => {
                let link = entity_summary::entity_link(&entity);
                (entity.name, link, true)
            }
            None => {
                let handle =
                    request::<BasicInfo>("user.basic_info.by_user_id", other_user_id.to_string())
                        .await
                        .map(|info| info.handle)
                        .unwrap_or_else(|_| "Unknown".to_string());
                (handle, format!("/profile/{}", other_user_id), false)
            }
        };

    let content = ThreadContent {
        topic_id: topic_ulid,
        subject: topic.title,
        other_user_handle,
        other_link,
        other_is_entity,
        posts: post_models,
        from_page: q.from.unwrap_or(0).max(0),
    };

    let layout = StandardLayout::new("/dashboard/messages", content)
        .prepare(Some(user_info))
        .await?;

    ThreadPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(Debug, Deserialize)]
pub struct ReplyForm {
    pub body: String,
}

#[handler]
pub async fn reply(
    Authenticated(user_info): Authenticated,
    Path(topic_id): Path<String>,
    Form(form): Form<ReplyForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let topic_ulid: Ulid = topic_id
        .parse()
        .map_err(|err| anyhow!("Invalid topic id: {}", err))?;

    if form.body.trim().is_empty() {
        return Err(anyhow!("Reply body is required"));
    }

    // A reply a user sends *to* a project/event counts toward the one-per-day
    // limit (the same project can always be continued; a different one can't).
    // The other participant is the conversation's non-author side.
    let topic: TopicInfo = request("forum.topics.get", topic_id.clone()).await?;
    let other_participant = if topic.author_id == user_info.user_id {
        topic.forum_id
    } else {
        topic.author_id
    };
    if let Some(entity) = entity_summary::lookup_batch(&[other_participant.to_string()])
        .await?
        .into_iter()
        .next()
    {
        if let Some(message) = daily_entity_limit_block(user_info.user_id, entity.id).await? {
            return Ok(limit_error_fragment(&message));
        }
    }

    let _: ReplyToMessageResponse = request(
        "forum.messaging.reply",
        ReplyToMessage {
            user_id: user_info.user_id,
            topic_id: topic_ulid,
            body: form.body,
            // A member's reply is held for staff review.
            pre_approved: false,
        },
    )
    .await?;

    // The reply is held for staff review, not delivered yet — tell the sender.
    // (The reply form posts via HTMX, so this renders inline.)
    Ok(pending_review_fragment())
}

#[handler]
pub async fn discard(
    Authenticated(user_info): Authenticated,
    Path(topic_id): Path<String>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let topic_ulid: Ulid = topic_id
        .parse()
        .map_err(|err| anyhow!("Invalid topic id: {}", err))?;

    nats_send(
        "forum.messaging.discard",
        DiscardMessage {
            user_id: user_info.user_id,
            topic_id: topic_ulid,
        },
    )
    .await?;

    Ok(Redirect::see_other("/dashboard/messages").into_response())
}

pub struct ConversationModel {
    pub topic_id: Ulid,
    pub subject: String,
    pub other_user_id: Ulid,
    pub other_user_handle: String,
    /// True when the correspondent is a project/event rather than a member,
    /// so the personal inbox can label it by name (no "@") instead of a handle.
    pub is_entity: bool,
    pub last_message_at: String,
    pub last_message_preview: String,
    pub post_count: i64,
    pub seen: bool,
}

impl From<ConversationSummary> for ConversationModel {
    fn from(c: ConversationSummary) -> Self {
        ConversationModel {
            topic_id: c.topic_id,
            subject: c.subject,
            other_user_id: c.other_user_id,
            other_user_handle: c.other_user_handle,
            is_entity: false,
            last_message_at: c.last_message_at.to_string(),
            last_message_preview: c.last_message_preview,
            post_count: c.post_count,
            seen: c.seen,
        }
    }
}

pub struct PostModel {
    pub is_self: bool,
    pub author_id: Ulid,
    pub author_handle: String,
    pub paragraphs: Vec<String>,
    pub created_at: String,
    /// False while this message is awaiting staff review. Only ever surfaced
    /// on the author's own messages (the recipient never receives an
    /// unapproved post), rendered as a "pending approval" badge.
    pub approved: bool,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/messages.stpl.html")]
struct InboxPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<InboxContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/messages_content.stpl.html")]
struct InboxContent {
    conversations: Vec<ConversationModel>,
    page: i64,
    has_prev_page: bool,
    has_next_page: bool,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/messages_thread.stpl.html")]
struct ThreadPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<ThreadContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/messages_thread_content.stpl.html")]
struct ThreadContent {
    topic_id: Ulid,
    subject: String,
    other_user_handle: String,
    /// Link to the correspondent — a member profile or a project/event page.
    other_link: String,
    /// True when the correspondent is a project/event (label by name, no "@").
    other_is_entity: bool,
    posts: Vec<PostModel>,
    /// The inbox page the user was on when they clicked into this thread,
    /// so the back link returns them there. 0 means "first page".
    from_page: i64,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/_partials/compose_form.stpl.html")]
struct ComposeFragment {
    target_user_id: Ulid,
    target_handle: String,
}
