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
    // Shares the `user-{user_id}` slug convention with forums::get_personal_forum.
    let slug = format!("user-{}", user_id_str);
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

    // Verify topic exists, lives in a personal forum, and the user is a participant
    let topic = sqlx::query!(
        r#"SELECT t."forum_id", t."author_id", t."is_deleted", f."slug" as "slug!: String"
           FROM "topics" t
           JOIN "forums" f ON f.forum_id = t.forum_id
           WHERE t."topic_id" = ?"#,
        topic_id_str,
    )
    .fetch_optional(db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("Topic not found"))?;

    if topic.is_deleted == 1 {
        return Err(anyhow::anyhow!("Conversation has been deleted"));
    }

    if !common::message::forum::is_personal_forum_slug(&topic.slug) {
        return Err(anyhow::anyhow!("Not a direct message"));
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
    // Peek one row beyond the requested page so we can reliably report
    // has_next_page without relying on an after-the-fact COUNT (which
    // can't account for the orphan filter applied in Rust below).
    let fetch_limit = limit + 1;

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

    // Restrict to personal-forum topics so community/general posts that the
    // user happens to have authored don't appear in their inbox. The
    // `user-%` slug is the canonical personal-forum marker (see migration
    // 20260525152321_normalize_personal_forum_slugs).
    //
    // The `seen` flag is computed direction-aware so dots in the UI match
    // what `unread_count` reports:
    //   - read if ms.seen = 1
    //   - read by default for outgoing conversations with no message_status
    //     row (the sender knows what they sent; only flips back to unread
    //     when the recipient replies and reply_to_message sets ms.seen = 0)
    //   - unread otherwise (incoming-never-opened, or seen=0)
    let rows = sqlx::query_as!(
        ConvRow,
        r#"SELECT t.topic_id as "topic_id!: String", t.title as "title!: String",
                  t.forum_id, t.author_id as "author_id!: String",
                  t.updated_at, t.created_at as "created_at!: String",
                  CASE
                    WHEN ms.seen = 1 THEN 1
                    WHEN ms.seen IS NULL AND t.author_id = ? AND t.forum_id <> ? THEN 1
                    ELSE 0
                  END as "seen!: i32",
                  COALESCE(ms.discarded, 0) as "discarded!: i32",
                  (SELECT COUNT(*) FROM posts p WHERE p.topic_id = t.topic_id AND p.is_deleted = 0) as "post_count!: i64",
                  (SELECT p.content FROM posts p WHERE p.topic_id = t.topic_id AND p.is_deleted = 0 ORDER BY p.created_at DESC LIMIT 1) as last_content
           FROM topics t
           JOIN forums f ON f.forum_id = t.forum_id
           LEFT JOIN message_status ms ON ms.user_id = ? AND ms.topic_id = t.topic_id
           WHERE t.is_deleted = 0
             AND f.slug LIKE 'user-%'
             AND (t.forum_id = ? OR t.author_id = ?)
             AND (? OR ms.discarded IS NULL OR ms.discarded = 0)
           ORDER BY COALESCE(t.updated_at, t.created_at) DESC
           LIMIT ? OFFSET ?"#,
        user_id_str,
        user_id_str,
        user_id_str,
        user_id_str,
        user_id_str,
        include_discarded,
        fetch_limit,
        offset,
    )
    .fetch_all(db)
    .await?;

    // If we got more rows than the user asked for, there's at least one
    // more page. Either way, slice down to the requested limit before
    // we render the visible page.
    let has_next_page = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let mut conversations = Vec::new();

    for row in rows {
        let forum_id = row.forum_id.unwrap_or_default();

        // Determine the "other" user in the conversation
        let other_user_id_str = if row.author_id == user_id_str {
            forum_id.clone()
        } else {
            row.author_id.clone()
        };

        // Skip orphaned conversations: if we can't resolve the other
        // participant (deleted account, or bogus author_id left over from
        // spam before the gating fixes) there's nothing useful to show.
        let Ok(other_user) =
            nats_request::<BasicInfo>("user.basic_info.by_user_id", &other_user_id_str).await
        else {
            continue;
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
            other_user_handle: other_user.handle,
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
        has_next_page,
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

    // The badge is a true cross-page total: with proper pagination on the
    // inbox, an unread message on page 3 still deserves to be advertised
    // by the badge. The cost is bounded by the SQL row scan (cheap, two
    // columns plus the LEFT JOIN) and one NATS round trip per distinct
    // correspondent across all of the user's unread.
    let rows = sqlx::query!(
        r#"SELECT t.forum_id as "forum_id!: String",
                  t.author_id as "author_id!: String",
                  ms.seen as "seen?: i64"
           FROM topics t
           JOIN forums f ON f.forum_id = t.forum_id
           LEFT JOIN message_status ms ON ms.user_id = ? AND ms.topic_id = t.topic_id
           WHERE t.is_deleted = 0
             AND f.slug LIKE 'user-%'
             AND (t.forum_id = ? OR t.author_id = ?)
             AND (ms.discarded IS NULL OR ms.discarded = 0)
             AND ((ms.seen IS NULL AND t.author_id <> ?) OR ms.seen = 0)"#,
        user_id_str,
        user_id_str,
        user_id_str,
        user_id_str,
    )
    .fetch_all(db())
    .await
    .unwrap_or_default();

    // Dedupe lookups so a chatty correspondent only costs one NATS round
    // trip even if they account for many unread conversations.
    use std::collections::{HashMap, HashSet};
    let mut other_ids: HashSet<String> = HashSet::new();
    let mut pairs: Vec<(String, String)> = Vec::with_capacity(rows.len());
    for row in rows {
        let other = if row.author_id == user_id_str {
            row.forum_id.clone()
        } else {
            row.author_id.clone()
        };
        other_ids.insert(other);
        pairs.push((row.forum_id, row.author_id));
    }

    let mut resolved: HashMap<String, bool> = HashMap::new();
    for id in &other_ids {
        let ok = nats_request::<BasicInfo>("user.basic_info.by_user_id", id)
            .await
            .is_ok();
        resolved.insert(id.clone(), ok);
    }

    let count = pairs
        .into_iter()
        .filter(|(forum_id, author_id)| {
            let other = if author_id == &user_id_str {
                forum_id
            } else {
                author_id
            };
            resolved.get(other).copied().unwrap_or(false)
        })
        .count() as i64;

    Ok(UnreadCountResponse { count })
}
