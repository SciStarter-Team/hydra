//! Manager-facing inbox for a project or event. A project/event is an
//! addressable message recipient (its inbox is a personal forum keyed by the
//! entity ULID); its managers read those messages and reply *as* the entity
//! here. Authorization uses the same page-editor tables the detail pages
//! and the owned-projects dashboard read: staff, or a page-editor row.

use std::sync::Arc;

use anyhow::{Error, anyhow};
use common::message::dashboard::{EntitySummary, EntitySummaryKind};
use common::message::forum::{
    InboxQuery, InboxResponse, MarkMessageRead, PostInfo, PostListQuery, ReplyToMessage,
    ReplyToMessageResponse, SendBulkMessage, SendBulkResponse, TopicInfo,
};
use common::message::user::BasicInfo;
use common::service::{request, send as nats_send};
use poem::{
    Response, handler,
    web::{Form, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::component::{Component, Head, StandardLayout};
use crate::dashboard::entity_summary;
use crate::dashboard::messages::{ConversationModel, PostModel};
use crate::local_time::{TimeFormat, local_time};
use crate::middleware::authorization::Authenticated;

const INBOX_PAGE_SIZE: i64 = 25;

/// Confirm the signed-in user manages `entity_id`, returning them plus the
/// entity's summary (name/kind) when they do, or `None` otherwise.
async fn authorize_manager(
    user_info: Option<Arc<BasicInfo>>,
    entity_id: &str,
) -> Result<Option<(Arc<BasicInfo>, EntitySummary)>, Error> {
    let Some(user) = user_info else {
        return Ok(None);
    };

    let Some(summary) = entity_summary::lookup_batch(&[entity_id.to_string()])
        .await?
        .into_iter()
        .next()
    else {
        return Ok(None);
    };

    let allowed = user.is_staff
        || user.is_superuser
        || crate::dashboard::manages_entity(&user.user_id.to_string(), &summary).await?;

    if allowed {
        Ok(Some((user, summary)))
    } else {
        Ok(None)
    }
}

fn forbidden() -> Error {
    anyhow!("You don't manage this project or event")
}

#[derive(Debug, Deserialize)]
pub struct InboxPageQuery {
    #[serde(default)]
    pub page: Option<i64>,
}

#[handler]
pub async fn inbox(
    Authenticated(user_info): Authenticated,
    Path(entity_id): Path<String>,
    Query(q): Query<InboxPageQuery>,
) -> Result<Response, Error> {
    let Some((user, summary)) = authorize_manager(user_info, &entity_id).await? else {
        return Err(forbidden());
    };
    let entity_ulid: Ulid = entity_id.parse().map_err(|e| anyhow!("Invalid id: {e}"))?;

    let page = q.page.unwrap_or(0).max(0);
    let offset = page * INBOX_PAGE_SIZE;

    let inbox: InboxResponse = request(
        "forum.messaging.inbox",
        InboxQuery {
            user_id: entity_ulid,
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

    // Only projects have participant data, so only they can broadcast.
    let can_broadcast = summary.kind == EntitySummaryKind::Project;

    let content = EntityInboxContent {
        entity_id,
        entity_name: summary.name,
        conversations,
        page,
        has_prev_page: page > 0,
        has_next_page: inbox.has_next_page,
        can_broadcast,
    };

    let layout = StandardLayout::new("/dashboard/owned", content)
        .prepare(Some(user))
        .await?;

    EntityInboxPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[handler]
pub async fn thread(
    Authenticated(user_info): Authenticated,
    Path((entity_id, topic_id)): Path<(String, String)>,
) -> Result<Response, Error> {
    let Some((user, summary)) = authorize_manager(user_info, &entity_id).await? else {
        return Err(forbidden());
    };
    let entity_ulid: Ulid = entity_id.parse().map_err(|e| anyhow!("Invalid id: {e}"))?;
    let topic_ulid: Ulid = topic_id
        .parse()
        .map_err(|e| anyhow!("Invalid topic id: {e}"))?;

    let topic: TopicInfo = request("forum.topics.get", topic_id.clone()).await?;
    // The conversation must live in this entity's inbox.
    if topic.forum_id != entity_ulid {
        return Err(anyhow!("This conversation is not in this entity's inbox"));
    }

    let posts: Vec<PostInfo> = request(
        "forum.posts.list",
        PostListQuery {
            topic_id: topic_ulid,
            limit: Some(200),
            offset: Some(0),
            // The entity is the viewer: managers see approved messages plus the
            // entity's own pending replies (badged), not members' pending ones.
            viewer_id: Some(entity_ulid),
        },
    )
    .await?;

    // Mark the conversation read for the entity.
    nats_send(
        "forum.messaging.read",
        MarkMessageRead {
            user_id: entity_ulid,
            topic_id: topic_ulid,
        },
    )
    .await?;

    let post_models: Vec<PostModel> = posts
        .into_iter()
        .map(|p| PostModel {
            // The entity's own replies render as "self".
            is_self: p.author_id == entity_ulid,
            author_id: p.author_id,
            author_handle: p.author_handle,
            paragraphs: p.content.split("\n\n").map(|s| s.to_string()).collect(),
            created_at: p.created_at.to_string(),
            approved: p.approved,
        })
        .collect();

    // The other participant is the external member who started the conversation.
    let other_user_id = topic.author_id;
    let other_user_handle =
        request::<BasicInfo>("user.basic_info.by_user_id", other_user_id.to_string())
            .await
            .map(|info| info.display_handle().to_string())
            .unwrap_or_else(|_| "Unknown".to_string());

    let content = EntityThreadContent {
        entity_id,
        entity_name: summary.name,
        topic_id: topic_ulid,
        subject: topic.title,
        other_user_id,
        other_user_handle,
        posts: post_models,
    };

    let layout = StandardLayout::new("/dashboard/owned", content)
        .prepare(Some(user))
        .await?;

    EntityThreadPage {
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
    Path((entity_id, topic_id)): Path<(String, String)>,
    Form(form): Form<ReplyForm>,
) -> Result<Response, Error> {
    let Some((_user, _summary)) = authorize_manager(user_info, &entity_id).await? else {
        return Err(forbidden());
    };
    let entity_ulid: Ulid = entity_id.parse().map_err(|e| anyhow!("Invalid id: {e}"))?;
    let topic_ulid: Ulid = topic_id
        .parse()
        .map_err(|e| anyhow!("Invalid topic id: {e}"))?;

    if form.body.trim().is_empty() {
        return Err(anyhow!("Reply body is required"));
    }

    let topic: TopicInfo = request("forum.topics.get", topic_id.clone()).await?;
    if topic.forum_id != entity_ulid {
        return Err(anyhow!("This conversation is not in this entity's inbox"));
    }

    // Reply *as* the entity: pass the entity ULID as the acting participant.
    // forum-service's participant check passes because the entity is the
    // inbox owner (forum_id). This is the entity answering a member, so the
    // one-per-day limit (which governs members contacting entities) does not
    // apply. It is a trusted, entity-authored message, so it is delivered
    // immediately (pre_approved) rather than held for staff review.
    let _: ReplyToMessageResponse = request(
        "forum.messaging.reply",
        ReplyToMessage {
            user_id: entity_ulid,
            topic_id: topic_ulid,
            body: form.body,
            pre_approved: true,
        },
    )
    .await?;

    // Delivered immediately — tell the sender (renders inline via HTMX).
    Ok(crate::dashboard::messages::sent_fragment(
        "Your reply has been sent.",
    ))
}

/// The participant (joined-member) user ULIDs for a project. Sourced from
/// user-service's `project_joins`. Ids that don't parse are dropped.
async fn project_participant_ids(entity_id: Ulid) -> Result<Vec<Ulid>, Error> {
    let ids: Vec<String> = request("user.joins.list_by_project_id", entity_id.to_string()).await?;
    Ok(ids.into_iter().filter_map(|s| s.parse().ok()).collect())
}

/// Confirm the caller manages `entity_id` *and* it is a project (only projects
/// have participant data today), returning the parsed id and entity summary.
async fn authorize_project_broadcaster(
    user_info: Option<Arc<BasicInfo>>,
    entity_id: &str,
) -> Result<(Arc<BasicInfo>, EntitySummary, Ulid), Error> {
    let Some((user, summary)) = authorize_manager(user_info, entity_id).await? else {
        return Err(forbidden());
    };
    if summary.kind != EntitySummaryKind::Project {
        return Err(anyhow!(
            "Messaging participants is only available for projects"
        ));
    }
    let entity_ulid: Ulid = entity_id.parse().map_err(|e| anyhow!("Invalid id: {e}"))?;
    Ok((user, summary, entity_ulid))
}

#[handler]
pub async fn broadcast_form(
    Authenticated(user_info): Authenticated,
    Path(entity_id): Path<String>,
) -> Result<Response, Error> {
    let (user, summary, entity_ulid) = authorize_project_broadcaster(user_info, &entity_id).await?;

    let participant_count = project_participant_ids(entity_ulid).await?.len();

    let content = BroadcastContent {
        entity_id,
        entity_name: summary.name,
        participant_count,
    };

    let layout = StandardLayout::new("/dashboard/owned", content)
        .prepare(Some(user))
        .await?;

    EntityBroadcastPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(Debug, Deserialize)]
pub struct BroadcastForm {
    pub subject: String,
    pub body: String,
}

#[handler]
pub async fn broadcast(
    Authenticated(user_info): Authenticated,
    Path(entity_id): Path<String>,
    Form(form): Form<BroadcastForm>,
) -> Result<Response, Error> {
    let (_user, _summary, entity_ulid) =
        authorize_project_broadcaster(user_info, &entity_id).await?;

    if form.subject.trim().is_empty() || form.body.trim().is_empty() {
        return Err(anyhow!("Subject and body are required"));
    }

    let participants = project_participant_ids(entity_ulid).await?;
    if participants.is_empty() {
        return Ok(crate::dashboard::messages::sent_fragment(
            "This project has no participants to message yet.",
        ));
    }

    // Send as the project to every participant. As an entity-authored message
    // it is delivered immediately (pre_approved); all copies share one batch.
    let resp: SendBulkResponse = request(
        "forum.messaging.send_bulk",
        SendBulkMessage {
            from_id: entity_ulid,
            to_ids: participants,
            subject: form.subject,
            body: form.body,
            pre_approved: true,
        },
    )
    .await?;

    Ok(crate::dashboard::messages::sent_fragment(&format!(
        "Sent to {} participant{}.",
        resp.sent,
        if resp.sent == 1 { "" } else { "s" },
    )))
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/entity_messages.stpl.html")]
struct EntityInboxPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<EntityInboxContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/entity_messages_content.stpl.html")]
struct EntityInboxContent {
    entity_id: String,
    entity_name: String,
    conversations: Vec<ConversationModel>,
    page: i64,
    has_prev_page: bool,
    has_next_page: bool,
    can_broadcast: bool,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/entity_messages_thread.stpl.html")]
struct EntityThreadPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<EntityThreadContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/entity_messages_thread_content.stpl.html")]
struct EntityThreadContent {
    entity_id: String,
    entity_name: String,
    topic_id: Ulid,
    subject: String,
    other_user_id: Ulid,
    other_user_handle: String,
    posts: Vec<PostModel>,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/entity_broadcast.stpl.html")]
struct EntityBroadcastPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<BroadcastContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/entity_broadcast_content.stpl.html")]
struct BroadcastContent {
    entity_id: String,
    entity_name: String,
    participant_count: usize,
}
