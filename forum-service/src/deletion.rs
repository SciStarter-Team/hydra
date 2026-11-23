//! Forgetting a deleted account's part in the forums, as orrery's forum and
//! messaging forget it (`common::user_deletions` says when).
//!
//! - Its conversations go whole. Each is between it and one other person, so
//!   every message in one was to it or from it, and orrery deletes a message
//!   with either end.
//! - Its personal forum (inbox) goes, and its standing in others'
//!   conversations with it.
//! - Its posts go. A reply to one stays, made a reply to the topic itself, as
//!   orrery's forum makes an answer to a deleted post stand on its own.
//! - A topic it opened goes, unless others have replied, when it keeps its
//!   title and their replies and loses the text the account opened it with.
//! - Its People Finder requests go, and its moderator appointments; the
//!   moderation log keeps what it did as a moderator.

use anyhow::Result;
use common::transaction::Immediate;
use sqlx::SqlitePool;
use ulid::Ulid;

/// Forget the deleted account `user_id`'s part in the forums.
pub async fn forget(pool: &SqlitePool, user_id: Ulid) -> Result<()> {
    let user = user_id.to_string();
    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    let conversations = sqlx::query_scalar!(
        r#"SELECT t."topic_id" AS "topic_id!" FROM "topics" t
           JOIN "forums" f ON f."forum_id" = t."forum_id"
           WHERE f."slug" LIKE 'inbox-%'
             AND (t."forum_id" = ?1 OR t."author_id" = ?1
                  OR EXISTS (SELECT 1 FROM "posts" p
                             WHERE p."topic_id" = t."topic_id" AND p."author_id" = ?1))"#,
        user
    )
    .fetch_all(&mut tx)
    .await?;
    for topic in &conversations {
        delete_topic(&mut tx, topic).await?;
    }

    sqlx::query!(
        r#"DELETE FROM "forum_moderators"
           WHERE "user_id" = ?1
              OR "forum_id" IN (SELECT "forum_id" FROM "forums"
                                WHERE "forum_id" = ?1 AND "slug" LIKE 'inbox-%')"#,
        user
    )
    .execute(&mut tx)
    .await?;
    sqlx::query!(
        r#"DELETE FROM "forums" WHERE "forum_id" = ? AND "slug" LIKE 'inbox-%'"#,
        user
    )
    .execute(&mut tx)
    .await?;

    sqlx::query!(
        r#"UPDATE "posts" SET "parent_post_id" = NULL
           WHERE "parent_post_id" IN (SELECT "post_id" FROM "posts" WHERE "author_id" = ?)"#,
        user
    )
    .execute(&mut tx)
    .await?;
    sqlx::query!(
        r#"DELETE FROM "orrery_messages"
           WHERE "post_id" IN (SELECT "post_id" FROM "posts" WHERE "author_id" = ?)"#,
        user
    )
    .execute(&mut tx)
    .await?;
    sqlx::query!(r#"DELETE FROM "posts" WHERE "author_id" = ?"#, user)
        .execute(&mut tx)
        .await?;

    let unanswered = sqlx::query_scalar!(
        r#"SELECT "topic_id" AS "topic_id!" FROM "topics" t
           WHERE "author_id" = ?
             AND NOT EXISTS (SELECT 1 FROM "posts" p WHERE p."topic_id" = t."topic_id")"#,
        user
    )
    .fetch_all(&mut tx)
    .await?;
    for topic in &unanswered {
        delete_topic(&mut tx, topic).await?;
    }
    sqlx::query!(
        r#"UPDATE "topics" SET "content" = '' WHERE "author_id" = ?"#,
        user
    )
    .execute(&mut tx)
    .await?;

    sqlx::query!(
        r#"DELETE FROM "people_finder_requests" WHERE "sender_id" = ?"#,
        user
    )
    .execute(&mut tx)
    .await?;
    sqlx::query!(r#"DELETE FROM "message_status" WHERE "user_id" = ?"#, user)
        .execute(&mut tx)
        .await?;

    tx.commit().await?;
    Ok(())
}

/// Delete the topic `topic` with everything in it.
async fn delete_topic(tx: &mut Immediate, topic: &str) -> Result<()> {
    sqlx::query!(
        r#"DELETE FROM "orrery_messages"
           WHERE "post_id" IN (SELECT "post_id" FROM "posts" WHERE "topic_id" = ?)"#,
        topic
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(r#"DELETE FROM "posts" WHERE "topic_id" = ?"#, topic)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(
        r#"DELETE FROM "message_reply_refs" WHERE "topic_id" = ?"#,
        topic
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"DELETE FROM "message_status" WHERE "topic_id" = ?"#,
        topic
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(r#"DELETE FROM "topic_tags" WHERE "topic_id" = ?"#, topic)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(r#"DELETE FROM "topics" WHERE "topic_id" = ?"#, topic)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    const ANN: &str = "01HZZZZZZZZZZZZZZZZZZZZANN";
    const BOB: &str = "01HZZZZZZZZZZZZZZZZZZZZBOB";
    const CAL: &str = "01HZZZZZZZZZZZZZZZZZZZZCAL";
    const PUBLIC: &str = "01HZZZZZZZZZZZZZZZZZZZGENR";

    async fn migrated() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        pool
    }

    async fn forum(pool: &SqlitePool, forum_id: &str, slug: &str) {
        sqlx::query(
            r#"INSERT INTO "forums" ("forum_id", "slug", "name", "created_at")
               VALUES (?, ?, ?, '2026-11-23T00:00:00Z')"#,
        )
        .bind(forum_id)
        .bind(slug)
        .bind(slug)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn topic(pool: &SqlitePool, topic_id: &str, forum_id: &str, author: &str) {
        sqlx::query(
            r#"INSERT INTO "topics" ("topic_id", "forum_id", "title", "content", "author_id", "created_at")
               VALUES (?, ?, 'Title', 'Opening text', ?, '2026-11-23T00:00:00Z')"#,
        )
        .bind(topic_id)
        .bind(forum_id)
        .bind(author)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn post(
        pool: &SqlitePool,
        post_id: &str,
        topic_id: &str,
        author: &str,
        parent: Option<&str>,
    ) {
        sqlx::query(
            r#"INSERT INTO "posts" ("post_id", "topic_id", "content", "author_id", "parent_post_id", "created_at", "approved")
               VALUES (?, ?, 'Reply', ?, ?, '2026-11-23T00:00:00Z', 1)"#,
        )
        .bind(post_id)
        .bind(topic_id)
        .bind(author)
        .bind(parent)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn ids(pool: &SqlitePool, sql: &'static str) -> Vec<String> {
        sqlx::query_scalar(sql).fetch_all(pool).await.unwrap()
    }

    #[tokio::test]
    async fn a_deleted_account_takes_its_conversations_and_posts_but_not_others_replies() {
        let pool = migrated().await;
        let ann = ANN.parse::<Ulid>().unwrap();

        forum(&pool, PUBLIC, "test-public").await;
        forum(&pool, ANN, &format!("inbox-{ANN}")).await;
        forum(&pool, BOB, &format!("inbox-{BOB}")).await;

        // Bob wrote to Ann, and Ann to Bob; Cal wrote to Bob.
        topic(&pool, "to-ann", ANN, BOB).await;
        post(&pool, "to-ann-1", "to-ann", BOB, None).await;
        topic(&pool, "to-bob", BOB, ANN).await;
        post(&pool, "to-bob-1", "to-bob", ANN, None).await;
        post(&pool, "to-bob-2", "to-bob", BOB, Some("to-bob-1")).await;
        topic(&pool, "cal-to-bob", BOB, CAL).await;
        post(&pool, "cal-to-bob-1", "cal-to-bob", CAL, None).await;

        // Ann opened a topic Bob answered, and one nobody did, and answered
        // Cal's topic, where Bob answered her.
        topic(&pool, "anns", PUBLIC, ANN).await;
        post(&pool, "anns-1", "anns", BOB, None).await;
        topic(&pool, "lonely", PUBLIC, ANN).await;
        topic(&pool, "cals", PUBLIC, CAL).await;
        post(&pool, "cals-1", "cals", ANN, None).await;
        post(&pool, "cals-2", "cals", BOB, Some("cals-1")).await;

        forget(&pool, ann).await.unwrap();

        check!(
            ids(&pool, r#"SELECT "topic_id" FROM "topics" ORDER BY 1"#).await
                == ["anns", "cal-to-bob", "cals"]
        );
        check!(
            ids(&pool, r#"SELECT "post_id" FROM "posts" ORDER BY 1"#).await
                == ["anns-1", "cal-to-bob-1", "cals-2"]
        );
        check!(
            ids(&pool, r#"SELECT "forum_id" FROM "forums" WHERE "slug" LIKE 'inbox-%' OR "slug" = 'test-public' ORDER BY 1"#).await
                == [PUBLIC, BOB]
        );
        // Bob's answer to Ann's post now answers Cal's topic itself.
        let parent: Option<String> = sqlx::query_scalar(
            r#"SELECT "parent_post_id" FROM "posts" WHERE "post_id" = 'cals-2'"#,
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        check!(parent.is_none());
        // Ann's answered topic keeps its title, without her opening text.
        let content: String =
            sqlx::query_scalar(r#"SELECT "content" FROM "topics" WHERE "topic_id" = 'anns'"#)
                .fetch_one(&pool)
                .await
                .unwrap();
        check!(content.is_empty());
    }
}
