use anyhow::{Error, anyhow};
use common::message::forum::{
    ConversationSummary, DiscardMessage, ForumInfo, GetForumById, InboxQuery, InboxResponse,
    MarkMessageRead, PostInfo, PostListQuery, ReplyToMessage, ReplyToMessageResponse, SendMessage,
    SendMessageResponse, TopicInfo, is_personal_forum_slug,
};
use common::message::user::BasicInfo;
use common::service::{request, send as nats_send};
use poem::{
    IntoResponse, Response, handler,
    http::StatusCode,
    session::Session,
    web::{Form, Html, Path, Query, RealIp, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::component::{Component, Head, StandardLayout};
use crate::local_time::{TimeFormat, local_time};
use crate::middleware::authorization::Authenticated;

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

    let conversations: Vec<ConversationModel> = inbox
        .conversations
        .into_iter()
        .map(ConversationModel::from)
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
    let to_user_id: Ulid = form
        .to
        .parse()
        .map_err(|err| anyhow!("Invalid recipient: {}", err))?;

    if form.subject.trim().is_empty() || form.body.trim().is_empty() {
        return Err(anyhow!("Subject and body are required"));
    }

    let resp: SendMessageResponse = request(
        "forum.messaging.send",
        SendMessage {
            from_user_id: user_info.user_id,
            to_user_id,
            subject: form.subject,
            body: form.body,
            reply_ref: None,
        },
    )
    .await?;

    let target = format!("/dashboard/messages/{}", resp.topic_id);
    Ok(Response::builder()
        .header("HX-Redirect", &target)
        .status(StatusCode::OK)
        .body(format!(
            "<p>Message sent. <a href=\"{}\">View conversation</a>.</p>",
            target
        )))
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
        })
        .collect();

    let other_user_id = if topic.author_id == user_info.user_id {
        topic.forum_id
    } else {
        topic.author_id
    };
    let other_user_handle =
        request::<BasicInfo>("user.basic_info.by_user_id", other_user_id.to_string())
            .await
            .map(|info| info.handle)
            .unwrap_or_else(|_| "Unknown".to_string());

    let content = ThreadContent {
        topic_id: topic_ulid,
        subject: topic.title,
        other_user_id,
        other_user_handle,
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

    let _: ReplyToMessageResponse = request(
        "forum.messaging.reply",
        ReplyToMessage {
            user_id: user_info.user_id,
            topic_id: topic_ulid,
            body: form.body,
        },
    )
    .await?;

    Ok(Redirect::see_other(format!("/dashboard/messages/{}", topic_id)).into_response())
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
    other_user_id: Ulid,
    other_user_handle: String,
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
