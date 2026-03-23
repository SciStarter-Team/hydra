use anyhow::Result;
use async_nats::service::Request;
use common::timestring::TimeString;
use common::{
    message::{forum::*, user::BasicInfo},
    service::request as nats_request,
    shared::db,
};
use ulid::Ulid;

async fn ensure_personal_forum(user_id: Ulid) -> Result<()> {
    let user_id_str = user_id.to_string();
    let slug = format!("inbox-{}", user_id_str);
    let name = format!("Inbox for {}", user_id_str);
    let now = TimeString::now().to_string();

    sqlx::query!(
        r#"INSERT OR IGNORE INTO "forums" ("forum_id", "slug", "name", "description", "created_at", "is_active", "display_order")
           VALUES (?, ?, ?, 'Personal message inbox', ?, 1, 0)"#,
        user_id_str,
        slug,
        name,
        now,
    )
    .execute(db())
    .await?;

    Ok(())
}

pub async fn send_message(req: &Request) -> Result<SendMessageResponse> {
    let msg: SendMessage = flexbuffers::from_slice(&req.message.payload)?;

    // Ensure recipient's personal forum exists
    ensure_personal_forum(msg.to_user_id).await?;

    let topic_id = Ulid::new();
    let post_id = Ulid::new();
    let now = TimeString::now();
    let now_str = now.to_string();
    let topic_id_str = topic_id.to_string();
    let post_id_str = post_id.to_string();
    let from_str = msg.from_user_id.to_string();
    let to_str = msg.to_user_id.to_string();

    let db = db();

    // Create topic in the recipient's personal forum (forum_id = to_user_id)
    sqlx::query!(
        r#"INSERT INTO "topics" ("topic_id", "forum_id", "title", "content", "author_id", "created_at", "updated_at", "is_pinned", "is_locked", "is_deleted", "is_flagged")
           VALUES (?, ?, ?, ?, ?, ?, ?, 0, 0, 0, 0)"#,
        topic_id_str,
        to_str,
        msg.subject,
        msg.body,
        from_str,
        now_str,
        now_str,
    )
    .execute(db)
    .await?;

    // Create the first post
    sqlx::query!(
        r#"INSERT INTO "posts" ("post_id", "topic_id", "content", "author_id", "created_at", "is_deleted", "is_flagged")
           VALUES (?, ?, ?, ?, ?, 0, 0)"#,
        post_id_str,
        topic_id_str,
        msg.body,
        from_str,
        now_str,
    )
    .execute(db)
    .await?;

    // Store reply_ref if provided (for orrery dedup)
    if let Some(ref reply_ref) = msg.reply_ref {
        sqlx::query!(
            r#"INSERT OR IGNORE INTO "message_reply_refs" ("topic_id", "reply_ref") VALUES (?, ?)"#,
            topic_id_str,
            reply_ref,
        )
        .execute(db)
        .await?;
    }

    // Mark as seen for the sender
    sqlx::query!(
        r#"INSERT OR REPLACE INTO "message_status" ("user_id", "topic_id", "seen", "discarded") VALUES (?, ?, 1, 0)"#,
        from_str,
        topic_id_str,
    )
    .execute(db)
    .await?;

    Ok(SendMessageResponse { topic_id, post_id })
}

pub async fn reply_to_message(req: &Request) -> Result<ReplyToMessageResponse> {
    let msg: ReplyToMessage = flexbuffers::from_slice(&req.message.payload)?;

    let post_id = Ulid::new();
    let now = TimeString::now();
    let now_str = now.to_string();
    let topic_id_str = msg.topic_id.to_string();
    let post_id_str = post_id.to_string();
    let user_id_str = msg.user_id.to_string();

    let db = db();

    // Verify topic exists and user is a participant
    let topic = sqlx::query!(
        r#"SELECT "forum_id", "author_id", "is_deleted" FROM "topics" WHERE "topic_id" = ?"#,
        topic_id_str,
    )
    .fetch_optional(db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("Topic not found"))?;

    if topic.is_deleted == 1 {
        return Err(anyhow::anyhow!("Conversation has been deleted"));
    }

    // User must be either the forum owner (recipient) or the topic author (sender)
    let forum_owner = topic.forum_id.as_deref().unwrap_or("");
    let topic_author = topic.author_id.as_str();
    if user_id_str != forum_owner && user_id_str != topic_author {
        return Err(anyhow::anyhow!("Not a participant in this conversation"));
    }

    // Create the reply post
    sqlx::query!(
        r#"INSERT INTO "posts" ("post_id", "topic_id", "content", "author_id", "created_at", "is_deleted", "is_flagged")
           VALUES (?, ?, ?, ?, ?, 0, 0)"#,
        post_id_str,
        topic_id_str,
        msg.body,
        user_id_str,
        now_str,
    )
    .execute(db)
    .await?;

    // Update topic's updated_at
    sqlx::query!(
        r#"UPDATE "topics" SET "updated_at" = ? WHERE "topic_id" = ?"#,
        now_str,
        topic_id_str,
    )
    .execute(db)
    .await?;

    // Mark as unread for the other participant
    let other_user = if user_id_str == forum_owner {
        topic_author.to_string()
    } else {
        forum_owner.to_string()
    };

    sqlx::query!(
        r#"INSERT INTO "message_status" ("user_id", "topic_id", "seen", "discarded")
           VALUES (?, ?, 0, 0)
           ON CONFLICT("user_id", "topic_id") DO UPDATE SET "seen" = 0"#,
        other_user,
        topic_id_str,
    )
    .execute(db)
    .await?;

    // Mark as seen for the replier
    sqlx::query!(
        r#"INSERT OR REPLACE INTO "message_status" ("user_id", "topic_id", "seen", "discarded") VALUES (?, ?, 1, 0)"#,
        user_id_str,
        topic_id_str,
    )
    .execute(db)
    .await?;

    Ok(ReplyToMessageResponse { post_id })
}

pub async fn inbox(req: &Request) -> Result<InboxResponse> {
    let query: InboxQuery = flexbuffers::from_slice(&req.message.payload)?;
    let user_id_str = query.user_id.to_string();
    let limit = query.limit.unwrap_or(25);
    let offset = query.offset.unwrap_or(0);

    let db = db();

    // Ensure personal forum exists
    ensure_personal_forum(query.user_id).await?;

    // Get conversations from the user's personal forum (where they are recipient)
    // plus conversations they started in others' personal forums (where they are author)
    let include_discarded: bool = query.include_discarded;

    struct ConvRow {
        topic_id: String,
        title: String,
        forum_id: Option<String>,
        author_id: String,
        updated_at: Option<String>,
        created_at: String,
        seen: i32,
        discarded: i32,
        post_count: i64,
        last_content: Option<String>,
    }

    let rows = sqlx::query_as!(
        ConvRow,
        r#"SELECT t.topic_id as "topic_id!: String", t.title as "title!: String",
                  t.forum_id, t.author_id as "author_id!: String",
                  t.updated_at, t.created_at as "created_at!: String",
                  COALESCE(ms.seen, 0) as "seen!: i32", COALESCE(ms.discarded, 0) as "discarded!: i32",
                  (SELECT COUNT(*) FROM posts p WHERE p.topic_id = t.topic_id AND p.is_deleted = 0) as "post_count!: i64",
                  (SELECT p.content FROM posts p WHERE p.topic_id = t.topic_id AND p.is_deleted = 0 ORDER BY p.created_at DESC LIMIT 1) as last_content
           FROM topics t
           LEFT JOIN message_status ms ON ms.user_id = ? AND ms.topic_id = t.topic_id
           WHERE t.is_deleted = 0
             AND (t.forum_id = ? OR t.author_id = ?)
             AND (? OR ms.discarded IS NULL OR ms.discarded = 0)
           ORDER BY COALESCE(t.updated_at, t.created_at) DESC
           LIMIT ? OFFSET ?"#,
        user_id_str,
        user_id_str,
        user_id_str,
        include_discarded,
        limit,
        offset,
    )
    .fetch_all(db)
    .await?;

    // Count total
    let total: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "cnt!: i64"
           FROM topics t
           LEFT JOIN message_status ms ON ms.user_id = ? AND ms.topic_id = t.topic_id
           WHERE t.is_deleted = 0
             AND (t.forum_id = ? OR t.author_id = ?)
             AND (? OR ms.discarded IS NULL OR ms.discarded = 0)"#,
        user_id_str,
        user_id_str,
        user_id_str,
        include_discarded,
    )
    .fetch_one(db)
    .await
    .unwrap_or(0);

    let mut conversations = Vec::new();

    for row in rows {
        let forum_id = row.forum_id.unwrap_or_default();

        // Determine the "other" user in the conversation
        let other_user_id_str = if row.author_id == user_id_str {
            forum_id.clone()
        } else {
            row.author_id.clone()
        };

        let other_user_handle =
            match nats_request::<BasicInfo>("user.basic_info.by_user_id", &other_user_id_str).await
            {
                Ok(info) => info.handle.clone(),
                Err(_) => other_user_id_str.clone(),
            };

        let last_message_at = row.updated_at.unwrap_or(row.created_at);
        let preview = row
            .last_content
            .unwrap_or_default()
            .chars()
            .take(100)
            .collect::<String>();

        conversations.push(ConversationSummary {
            topic_id: row.topic_id.parse().unwrap_or_default(),
            subject: row.title,
            other_user_id: other_user_id_str.parse().unwrap_or_default(),
            other_user_handle,
            last_message_at: last_message_at
                .try_into()
                .unwrap_or_else(|_| TimeString::now()),
            last_message_preview: preview,
            post_count: row.post_count,
            seen: row.seen != 0,
            discarded: row.discarded != 0,
        });
    }

    Ok(InboxResponse {
        conversations,
        total,
    })
}

pub async fn mark_read(req: &Request) -> Result<(), anyhow::Error> {
    let msg: MarkMessageRead = flexbuffers::from_slice(&req.message.payload)?;
    let user_id_str = msg.user_id.to_string();
    let topic_id_str = msg.topic_id.to_string();

    sqlx::query!(
        r#"INSERT INTO "message_status" ("user_id", "topic_id", "seen", "discarded")
           VALUES (?, ?, 1, 0)
           ON CONFLICT("user_id", "topic_id") DO UPDATE SET "seen" = 1"#,
        user_id_str,
        topic_id_str,
    )
    .execute(db())
    .await?;

    Ok(())
}

pub async fn discard(req: &Request) -> Result<(), anyhow::Error> {
    let msg: DiscardMessage = flexbuffers::from_slice(&req.message.payload)?;
    let user_id_str = msg.user_id.to_string();
    let topic_id_str = msg.topic_id.to_string();

    sqlx::query!(
        r#"INSERT INTO "message_status" ("user_id", "topic_id", "seen", "discarded")
           VALUES (?, ?, 1, 1)
           ON CONFLICT("user_id", "topic_id") DO UPDATE SET "seen" = 1, "discarded" = 1"#,
        user_id_str,
        topic_id_str,
    )
    .execute(db())
    .await?;

    Ok(())
}

pub async fn unread_count(req: &Request) -> Result<UnreadCountResponse> {
    let msg: UnreadCountRequest = flexbuffers::from_slice(&req.message.payload)?;
    let user_id_str = msg.user_id.to_string();

    let count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "cnt!: i64"
           FROM topics t
           LEFT JOIN message_status ms ON ms.user_id = ? AND ms.topic_id = t.topic_id
           WHERE t.is_deleted = 0
             AND (t.forum_id = ? OR t.author_id = ?)
             AND (ms.seen IS NULL OR ms.seen = 0)
             AND (ms.discarded IS NULL OR ms.discarded = 0)"#,
        user_id_str,
        user_id_str,
        user_id_str,
    )
    .fetch_one(db())
    .await
    .unwrap_or(0);

    Ok(UnreadCountResponse { count })
}
