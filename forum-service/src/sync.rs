use anyhow::Result;
use common::{misc, shared::db, timestring::TimeString};
use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;
use ulid::Ulid;

#[derive(Debug, Deserialize)]
struct MessageData {
    id: i32,
    from_ulid: Option<String>,
    to_ulid: Option<String>,
    subject: String,
    body: String,
    sent: String,
    seen: bool,
    discarded: bool,
    #[allow(dead_code)]
    group: Option<String>,
    in_reply_to_reply_ref: Option<String>,
    reply_ref: String,
}

pub async fn synchronize_messages() {
    loop {
        let last: String = misc::get("message-sync-bookmark-sent")
            .await
            .expect("Retrieving sync bookmark failed")
            .unwrap_or_else(|| String::from("1900-01-01T00:00:00Z"));
        let last_id: i32 = misc::get("message-sync-last-id")
            .await
            .expect("Retrieving sync last_id failed")
            .unwrap_or(0);

        let client = reqwest::Client::new();

        let messages: Vec<MessageData> = match client
            .get("https://scistarter.org/staff/sync/messages")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&[("since", &last), ("last_id", &last_id.to_string())])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => {
                    let response_text = match resp.text().await {
                        Ok(text) => text,
                        Err(e) => {
                            eprintln!("Failed to read response body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    };

                    match serde_json::from_str(&response_text) {
                        Ok(msgs) => msgs,
                        Err(e) => {
                            eprintln!("JSON parsing error: {}", e);
                            eprintln!("Response body: {}", response_text);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("HTTP error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Request error: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let mut bookmark = last.clone();
        let mut bookmark_id = last_id;

        let got_messages = messages.len();

        for msg in messages {
            if msg.sent > bookmark || (msg.sent == bookmark && msg.id > bookmark_id) {
                bookmark = msg.sent.clone();
                bookmark_id = msg.id;
            }

            if let Err(e) = insert_message(&msg).await {
                eprintln!("Failed to insert message {}: {}", msg.id, e);
            }
        }

        misc::set("message-sync-bookmark-sent", bookmark)
            .await
            .expect("Saving sync bookmark failed");
        misc::set("message-sync-last-id", bookmark_id)
            .await
            .expect("Saving sync last_id failed");

        if got_messages == 500 {
            sleep(Duration::from_secs(60)).await;
        } else {
            sleep(Duration::from_secs(60 * 60)).await;
        }
    }
}

async fn insert_message(msg: &MessageData) -> Result<()> {
    let db = db();

    // Check if we've already imported this message via reply_ref
    let existing = sqlx::query_scalar!(
        r#"SELECT "topic_id" FROM "message_reply_refs" WHERE "reply_ref" = ?"#,
        msg.reply_ref,
    )
    .fetch_optional(db)
    .await?;

    if existing.is_some() {
        return Ok(());
    }

    let from_ulid = match &msg.from_ulid {
        Some(u) => u.as_str(),
        None => return Ok(()),
    };
    let to_ulid = match &msg.to_ulid {
        Some(u) => u.as_str(),
        None => return Ok(()),
    };

    // Determine if this is a reply or a new conversation
    if let Some(ref parent_reply_ref) = msg.in_reply_to_reply_ref {
        let parent_topic_id = sqlx::query_scalar!(
            r#"SELECT "topic_id" FROM "message_reply_refs" WHERE "reply_ref" = ?"#,
            parent_reply_ref,
        )
        .fetch_optional(db)
        .await?;

        if let Some(topic_id_str) = parent_topic_id {
            // Add as a reply post to existing topic
            let post_id = Ulid::new().to_string();

            sqlx::query!(
                r#"INSERT INTO "posts" ("post_id", "topic_id", "content", "author_id", "created_at", "is_deleted", "is_flagged", "approved")
                   VALUES (?, ?, ?, ?, ?, 0, 0, 1)"#,
                post_id,
                topic_id_str,
                msg.body,
                from_ulid,
                msg.sent,
            )
            .execute(db)
            .await?;

            sqlx::query!(
                r#"UPDATE "topics" SET "updated_at" = ? WHERE "topic_id" = ?"#,
                msg.sent,
                topic_id_str,
            )
            .execute(db)
            .await?;

            sqlx::query!(
                r#"INSERT OR IGNORE INTO "message_reply_refs" ("topic_id", "reply_ref") VALUES (?, ?)"#,
                topic_id_str,
                msg.reply_ref,
            )
            .execute(db)
            .await?;

            update_message_status(
                db,
                from_ulid,
                to_ulid,
                &topic_id_str,
                msg.seen,
                msg.discarded,
            )
            .await?;

            return Ok(());
        }
    }

    // New conversation: create personal forum, topic, and first post
    ensure_personal_forum(to_ulid).await?;

    let topic_id = Ulid::new().to_string();
    let post_id = Ulid::new().to_string();

    sqlx::query!(
        r#"INSERT INTO "topics" ("topic_id", "forum_id", "title", "content", "author_id", "created_at", "updated_at", "is_pinned", "is_locked", "is_deleted", "is_flagged")
           VALUES (?, ?, ?, ?, ?, ?, ?, 0, 0, 0, 0)"#,
        topic_id,
        to_ulid,
        msg.subject,
        msg.body,
        from_ulid,
        msg.sent,
        msg.sent,
    )
    .execute(db)
    .await?;

    sqlx::query!(
        r#"INSERT INTO "posts" ("post_id", "topic_id", "content", "author_id", "created_at", "is_deleted", "is_flagged", "approved")
           VALUES (?, ?, ?, ?, ?, 0, 0, 1)"#,
        post_id,
        topic_id,
        msg.body,
        from_ulid,
        msg.sent,
    )
    .execute(db)
    .await?;

    sqlx::query!(
        r#"INSERT OR IGNORE INTO "message_reply_refs" ("topic_id", "reply_ref") VALUES (?, ?)"#,
        topic_id,
        msg.reply_ref,
    )
    .execute(db)
    .await?;

    update_message_status(db, from_ulid, to_ulid, &topic_id, msg.seen, msg.discarded).await?;

    Ok(())
}

async fn update_message_status(
    db: &sqlx::SqlitePool,
    from_ulid: &str,
    to_ulid: &str,
    topic_id: &str,
    seen: bool,
    discarded: bool,
) -> Result<()> {
    sqlx::query!(
        r#"INSERT OR REPLACE INTO "message_status" ("user_id", "topic_id", "seen", "discarded") VALUES (?, ?, 1, 0)"#,
        from_ulid,
        topic_id,
    )
    .execute(db)
    .await?;

    let seen_int: i32 = if seen { 1 } else { 0 };
    let discarded_int: i32 = if discarded { 1 } else { 0 };

    sqlx::query!(
        r#"INSERT OR REPLACE INTO "message_status" ("user_id", "topic_id", "seen", "discarded") VALUES (?, ?, ?, ?)"#,
        to_ulid,
        topic_id,
        seen_int,
        discarded_int,
    )
    .execute(db)
    .await?;

    Ok(())
}

async fn ensure_personal_forum(user_id: &str) -> Result<()> {
    let slug = format!("inbox-{}", user_id);
    let name = format!("Inbox for {}", user_id);
    let now = TimeString::now().to_string();

    sqlx::query!(
        r#"INSERT OR IGNORE INTO "forums" ("forum_id", "slug", "name", "description", "created_at", "is_active", "display_order")
           VALUES (?, ?, ?, 'Personal message inbox', ?, 1, 0)"#,
        user_id,
        slug,
        name,
        now,
    )
    .execute(db())
    .await?;

    Ok(())
}
