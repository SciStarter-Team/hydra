//! The copy of orrery's direct messages kept until cutover, as personal-forum
//! conversations.
//!
//! Messages come a page at a time from `/staff/sync/messages`. That feed
//! pages in the order orrery last changed its messages, so a message comes
//! again whenever it is seen, discarded, reviewed or edited, and its copy is
//! brought up to date. Orrery stamps that change when it writes a row, not
//! when the write is committed, so a row can appear behind the cursor; each
//! round reads again from a few minutes behind where the last ended, and
//! copying a message twice changes nothing. Deletions are found by comparing
//! every id orrery still has (`/staff/sync/message_ids`), and delete the
//! copy as staff review deletes a message, hidden from everyone.
//!
//! Before orrery's feed sends `changed` it pages by when a message was sent,
//! and is read with that cursor. The first page that sends it moves the sync
//! onto the new cursor, which starts from the beginning: one pass over every
//! message, bringing every copy up to date.
//!
//! A copy is keyed on orrery's id for its message. Copies made before they
//! carried it are claimed by the conversation their reply reference (or the
//! one they answer) is in, and their sender, time and body, the first time
//! orrery sends them; those no message claims, by the end of the first
//! round that reads every message, are of messages orrery has since
//! deleted, and are deleted.
//!
//! A conversation is between two participants, and holds a message and
//! everything between the same two that answers it, however the messages
//! come: an answer to a message not yet copied starts a conversation of its
//! own, which is folded into its message's when that comes. Orrery's
//! threads are not so confined: an answer may be copied to a third party,
//! sent from a manager's own box to a message the project's box received,
//! or name whatever message its sender chose. An answer between another two
//! goes in a conversation of theirs, as any message does whose thread has
//! none of them yet.
//!
//! Nothing here notifies anyone: a copy is orrery's message, which orrery
//! has already delivered or held.

use std::collections::{BTreeSet, HashSet};
use std::time::Duration;

use anyhow::{Result, anyhow};
use common::{misc, primary::CopyKeeper, shared::db, transaction::Immediate};
use serde::Deserialize;
use sqlx::SqlitePool;
use tokio::time::sleep;
use ulid::Ulid;

use crate::messaging::ensure_personal_forum;

/// How far behind its cursor each round reads messages again.
const OVERLAP: jiff::SignedDuration = jiff::SignedDuration::from_mins(5);

/// Orrery's page of messages holds at most this many.
const PAGE: usize = 500;

/// The pause between one full page and the next.
const BETWEEN_PAGES: Duration = Duration::from_secs(5);

/// The pause between rounds, each of which reads to the end of the feed.
const BETWEEN_ROUNDS: Duration = Duration::from_secs(60 * 60);

/// The pause after a round that failed.
const AFTER_FAILURE: Duration = Duration::from_secs(60);

/// How often the copies of messages orrery has deleted are deleted.
const SWEEP_EVERY: jiff::SignedDuration = jiff::SignedDuration::from_hours(24);

/// The most of the copies one sweep may delete, as a fraction (1 in this
/// many). Orrery deletes a message now and then; a list of ids that would
/// take more with it is a fault on its side, and is refused.
const SWEEP_LIMIT: usize = 100;

const LEGACY_SENT: &str = "message-sync-bookmark-sent";
const LEGACY_ID: &str = "message-sync-last-id";
const CURSOR_CHANGED: &str = "message-sync-changed";
const CURSOR_ID: &str = "message-sync-changed-id";
const SWEPT: &str = "message-sync-swept";

/// Set once a round has read every message orrery has, and so claimed every
/// copy that has a message to claim it.
const CLAIMED: &str = "message-sync-claimed";

/// Set once the copies placed in their thread's conversation although they
/// went between another two have been moved to conversations of their own.
const SEPARATED: &str = "message-sync-pairs-separated";

/// What orrery's review made of a message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ReviewStatus {
    /// Held for review.
    Pending,

    /// Delivered: reviewed and passed, or never held. A feed that doesn't
    /// say sends only these.
    #[default]
    Approved,

    /// Refused, and so never delivered.
    Denied,
}

impl ReviewStatus {
    /// The `approved` and `is_deleted` of the copy, as hydra's own review
    /// leaves a message it holds, passes or refuses.
    fn post_state(self) -> (bool, bool) {
        match self {
            ReviewStatus::Pending => (false, false),
            ReviewStatus::Approved => (true, false),
            ReviewStatus::Denied => (false, true),
        }
    }
}

/// One of orrery's messages, as `/staff/sync/messages` sends it.
#[derive(Debug, Clone, Deserialize)]
struct OrreryMessage {
    id: i32,
    from_ulid: Option<String>,
    to_ulid: Option<String>,
    subject: String,
    body: String,
    sent: String,
    seen: bool,
    discarded: bool,
    in_reply_to_reply_ref: Option<String>,
    reply_ref: String,
    #[serde(default)]
    review_status: ReviewStatus,
    /// When orrery last changed the message, which its feed pages by. A
    /// feed that pages by when it was sent doesn't send it.
    #[serde(default)]
    changed: Option<String>,
}

/// Copy orrery's messages, until hydra keeps its own.
pub async fn synchronize_messages() {
    if !CopyKeeper::NOW.orrery_keeps_copies() {
        return;
    }

    let client = reqwest::Client::new();

    loop {
        let pause = match round(&client).await {
            Ok(()) => BETWEEN_ROUNDS,
            Err(e) => {
                eprintln!("Message sync error: {e:#}");
                AFTER_FAILURE
            }
        };

        sleep(pause).await;
    }
}

/// Copy every message changed since the cursor, and once a day delete the
/// copies of those orrery no longer has.
///
/// A message that cannot be copied ends the round there, with the cursor
/// left before its page, so that it is tried again rather than passed over.
async fn round(client: &reqwest::Client) -> Result<()> {
    if misc::get::<bool>(SEPARATED).await?.is_none() {
        separate_pairs(db()).await?;
        misc::set(SEPARATED, true).await?;
    }

    if cursor().await?.is_none() && !legacy_round(client).await? {
        return Ok(());
    }

    changed_round(client).await?;

    if misc::get::<bool>(CLAIMED).await?.is_none() {
        delete_unclaimed(db()).await?;
        misc::set(CLAIMED, true).await?;
    }

    let swept: Option<String> = misc::get(SWEPT).await?;
    let swept = swept
        .map(|swept| swept.parse::<jiff::Timestamp>())
        .transpose()?;
    let now = jiff::Timestamp::now();
    if swept.is_none_or(|swept| now - SWEEP_EVERY >= swept) {
        sweep(client).await?;
        misc::set(SWEPT, now.to_string()).await?;
    }

    Ok(())
}

/// Read the feed as it paged before it sent `changed`: by when a message
/// was sent. Whether it sends `changed` now, and so is to be read by it.
async fn legacy_round(client: &reqwest::Client) -> Result<bool> {
    loop {
        let mut bookmark: String = misc::get(LEGACY_SENT)
            .await?
            .unwrap_or_else(|| String::from("1900-01-01T00:00:00Z"));
        let mut bookmark_id: i32 = misc::get(LEGACY_ID).await?.unwrap_or(0);

        let page: Vec<OrreryMessage> = client
            .get("https://scistarter.org/staff/sync/messages")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&[("since", &bookmark), ("last_id", &bookmark_id.to_string())])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        if page.iter().any(|message| message.changed.is_some()) {
            return Ok(true);
        }

        for message in &page {
            import(db(), message)
                .await
                .map_err(|e| anyhow!("message {}: {e:#}", message.id))?;

            if (&message.sent, message.id) > (&bookmark, bookmark_id) {
                bookmark = message.sent.clone();
                bookmark_id = message.id;
            }
        }

        misc::set(LEGACY_SENT, bookmark).await?;
        misc::set(LEGACY_ID, bookmark_id).await?;

        if page.len() < PAGE {
            return Ok(false);
        }

        sleep(BETWEEN_PAGES).await;
    }
}

/// Read the feed by when orrery last changed each message, from a little
/// behind the cursor (or from the beginning, when there is none) to the end.
async fn changed_round(client: &reqwest::Client) -> Result<()> {
    let mut since = cursor().await?.map(|(changed, _)| changed - OVERLAP);
    let mut last_id = 0;

    loop {
        let mut query = vec![("last_id", last_id.to_string())];
        if let Some(since) = since {
            query.push((
                "since",
                since.strftime("%Y-%m-%dT%H:%M:%S%.f+00:00").to_string(),
            ));
        }

        let page: Vec<OrreryMessage> = client
            .get("https://scistarter.org/staff/sync/messages")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&query)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        for message in &page {
            let changed = message
                .changed
                .as_deref()
                .ok_or_else(|| anyhow!("message {} came without `changed`", message.id))?
                .parse()?;

            import(db(), message)
                .await
                .map_err(|e| anyhow!("message {}: {e:#}", message.id))?;

            since = Some(changed);
            last_id = message.id;
        }

        if let (Some(changed), Some(_)) = (since, page.last()) {
            advance_cursor(changed, last_id).await?;
        }

        if page.len() < PAGE {
            return Ok(());
        }

        sleep(BETWEEN_PAGES).await;
    }
}

/// Where the last round on the `changed` cursor stopped: when orrery last
/// changed the last message it copied, and that message's id. `None` until
/// the sync has read a page by it.
async fn cursor() -> Result<Option<(jiff::Timestamp, i32)>> {
    let changed: Option<String> = misc::get(CURSOR_CHANGED).await?;
    let id: Option<i32> = misc::get(CURSOR_ID).await?;

    Ok(changed
        .map(|changed| changed.parse())
        .transpose()?
        .map(|changed| (changed, id.unwrap_or(0))))
}

/// Move the cursor on to `changed` and `id`, unless it is past them already,
/// as it is while a round reads again behind it.
async fn advance_cursor(changed: jiff::Timestamp, id: i32) -> Result<()> {
    if cursor().await?.is_some_and(|kept| kept >= (changed, id)) {
        return Ok(());
    }

    misc::set(CURSOR_CHANGED, changed.to_string()).await?;
    misc::set(CURSOR_ID, id).await?;

    Ok(())
}

/// Delete the copies of the messages orrery no longer has.
async fn sweep(client: &reqwest::Client) -> Result<()> {
    // The copies are listed before orrery's ids are read, so that a message
    // copied meanwhile isn't taken for one it lacks.
    let copied = live_copies(db()).await?;

    let mut present = HashSet::new();
    let mut last_id = 0;
    loop {
        let page: Vec<i32> = client
            .get("https://scistarter.org/staff/sync/message_ids")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&[("last_id", last_id.to_string())])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let Some(&last) = page.last() else { break };
        if page.first().is_some_and(|&first| first <= last_id)
            || page.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(anyhow!(
                "orrery's message ids after {last_id} are out of order"
            ));
        }

        present.extend(page);
        last_id = last;
    }

    delete_absent(db(), &copied, &present).await
}

/// Orrery's ids for the messages whose copies aren't deleted.
async fn live_copies(pool: &SqlitePool) -> Result<Vec<i32>> {
    Ok(sqlx::query_scalar!(
        r#"SELECT om."orrery_id" AS "orrery_id!: i32"
           FROM "orrery_messages" om
           JOIN "posts" p ON p."post_id" = om."post_id"
           WHERE p."is_deleted" = 0"#
    )
    .fetch_all(pool)
    .await?)
}

/// Delete the copies, among those of the messages `copied`, of the
/// messages not `present` in orrery.
async fn delete_absent(pool: &SqlitePool, copied: &[i32], present: &HashSet<i32>) -> Result<()> {
    let absent: Vec<i32> = copied
        .iter()
        .copied()
        .filter(|id| !present.contains(id))
        .collect();

    if absent.len() * SWEEP_LIMIT > copied.len() {
        return Err(anyhow!(
            "orrery's message ids would delete {} of {} copies",
            absent.len(),
            copied.len()
        ));
    }

    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;
    let mut topics = BTreeSet::new();

    for id in absent {
        let topic = sqlx::query_scalar!(
            r#"UPDATE "posts" SET "is_deleted" = 1
               WHERE "post_id" = (SELECT "post_id" FROM "orrery_messages" WHERE "orrery_id" = ?)
               RETURNING "topic_id""#,
            id
        )
        .fetch_optional(&mut tx)
        .await?;
        topics.extend(topic);
    }

    for topic in topics {
        update_statuses(&mut tx, &topic).await?;
    }

    tx.commit().await?;

    Ok(())
}

/// Delete the copies no message claimed in a round that read every message
/// orrery has: copies, made before copies were linked, of messages it has
/// since deleted. They are the posts of the conversations copied from
/// orrery (those a reply reference is in) that no message is linked to.
///
/// A count of them that would take more than one copy in a hundred
/// ([`SWEEP_LIMIT`]) is a fault in claiming them, and is refused.
async fn delete_unclaimed(pool: &SqlitePool) -> Result<()> {
    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    let counts = sqlx::query!(
        r#"SELECT COUNT(*) AS "copies!: i64",
                  COUNT(*) FILTER (WHERE om."post_id" IS NULL) AS "unclaimed!: i64"
           FROM "posts" p
           LEFT JOIN "orrery_messages" om ON om."post_id" = p."post_id"
           WHERE p."is_deleted" = 0
             AND p."topic_id" IN (SELECT "topic_id" FROM "message_reply_refs")"#
    )
    .fetch_one(&mut tx)
    .await?;

    if counts.unclaimed * SWEEP_LIMIT as i64 > counts.copies {
        return Err(anyhow!(
            "{} of {} copies of orrery's messages are unclaimed; refusing to delete them",
            counts.unclaimed,
            counts.copies
        ));
    }

    let topics: BTreeSet<String> = sqlx::query_scalar!(
        r#"UPDATE "posts" SET "is_deleted" = 1
           WHERE "is_deleted" = 0
             AND "topic_id" IN (SELECT "topic_id" FROM "message_reply_refs")
             AND "post_id" NOT IN (SELECT "post_id" FROM "orrery_messages")
           RETURNING "topic_id""#
    )
    .fetch_all(&mut tx)
    .await?
    .into_iter()
    .collect();

    for topic in topics {
        update_statuses(&mut tx, &topic).await?;
    }

    tx.commit().await?;

    Ok(())
}

/// Copy `message`, or bring its copy up to date. A message with no sender
/// or no recipient (whose account is gone) has nowhere to go, and is passed
/// over.
async fn import(pool: &SqlitePool, message: &OrreryMessage) -> Result<()> {
    let (Some(sender), Some(recipient)) =
        (message.from_ulid.as_deref(), message.to_ulid.as_deref())
    else {
        return Ok(());
    };

    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    let linked = sqlx::query_scalar!(
        r#"SELECT "post_id" FROM "orrery_messages" WHERE "orrery_id" = ?"#,
        message.id
    )
    .fetch_optional(&mut tx)
    .await?;
    let post_id = match linked {
        Some(post_id) => post_id,
        None => place(&mut tx, message, sender, recipient).await?,
    };

    let (approved, deleted) = message.review_status.post_state();
    let topic = sqlx::query_scalar!(
        r#"UPDATE "posts" SET "approved" = ?, "is_deleted" = ? WHERE "post_id" = ?
           RETURNING "topic_id""#,
        approved,
        deleted,
        post_id
    )
    .fetch_one(&mut tx)
    .await?;

    sqlx::query!(
        r#"INSERT INTO "orrery_messages"
               ("orrery_id", "post_id", "reply_ref", "parent_reply_ref", "sender_id",
                "recipient_id", "sent", "seen", "discarded")
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT ("orrery_id") DO UPDATE SET
               "reply_ref" = excluded."reply_ref",
               "parent_reply_ref" = excluded."parent_reply_ref",
               "sender_id" = excluded."sender_id",
               "recipient_id" = excluded."recipient_id",
               "sent" = excluded."sent",
               "seen" = excluded."seen",
               "discarded" = excluded."discarded""#,
        message.id,
        post_id,
        message.reply_ref,
        message.in_reply_to_reply_ref,
        sender,
        recipient,
        message.sent,
        message.seen,
        message.discarded,
    )
    .execute(&mut tx)
    .await?;

    sqlx::query!(
        r#"INSERT INTO "message_reply_refs" ("reply_ref", "topic_id") VALUES (?, ?)
           ON CONFLICT ("reply_ref") DO NOTHING"#,
        message.reply_ref,
        topic,
    )
    .execute(&mut tx)
    .await?;

    let topic = gather(&mut tx, message, sender, recipient, topic).await?;
    update_statuses(&mut tx, &topic).await?;

    tx.commit().await?;

    Ok(())
}

/// The post that copies `message`, which has none linked to it yet: one
/// made before copies were linked, or else a new one. It goes in the
/// conversation its reply reference is in, or else the one of the message
/// it answers, so long as that conversation is between its sender and
/// recipient, or else a new one in its recipient's inbox.
async fn place(
    tx: &mut Immediate,
    message: &OrreryMessage,
    sender: &str,
    recipient: &str,
) -> Result<String> {
    let mut topic = conversation_of(tx, &message.reply_ref, sender, recipient).await?;
    if topic.is_none()
        && let Some(parent) = &message.in_reply_to_reply_ref
    {
        topic = conversation_of(tx, parent, sender, recipient).await?;
    }

    let topic = match topic {
        Some(topic) => topic,
        None => {
            start_conversation(
                tx,
                sender,
                recipient,
                &message.subject,
                &message.body,
                &message.sent,
            )
            .await?
        }
    };

    let earlier = sqlx::query_scalar!(
        r#"SELECT p."post_id" AS "post_id!: String" FROM "posts" p
           WHERE p."topic_id" = ? AND p."author_id" = ? AND p."created_at" = ? AND p."content" = ?
             AND NOT EXISTS (SELECT 1 FROM "orrery_messages" om WHERE om."post_id" = p."post_id")
           ORDER BY p."post_id"
           LIMIT 1"#,
        topic,
        sender,
        message.sent,
        message.body,
    )
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(post_id) = earlier {
        return Ok(post_id);
    }

    let post_id = Ulid::new().to_string();
    sqlx::query!(
        r#"INSERT INTO "posts" ("post_id", "topic_id", "content", "author_id", "created_at", "is_deleted", "is_flagged", "approved")
           VALUES (?, ?, ?, ?, ?, 0, 0, 0)"#,
        post_id,
        topic,
        message.body,
        sender,
        message.sent,
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        r#"UPDATE "topics" SET "updated_at" = ?
           WHERE "topic_id" = ? AND COALESCE("updated_at", "created_at") < ?"#,
        message.sent,
        topic,
        message.sent,
    )
    .execute(&mut *tx)
    .await?;

    Ok(post_id)
}

/// The conversation the message whose reply reference is `reply_ref` is
/// in, if it has been copied and the conversation is between `one` and
/// `other`. A conversation is between the author of its topic and the owner
/// of the inbox it is in.
async fn conversation_of(
    tx: &mut Immediate,
    reply_ref: &str,
    one: &str,
    other: &str,
) -> Result<Option<String>> {
    Ok(sqlx::query_scalar!(
        r#"SELECT r."topic_id" FROM "message_reply_refs" r
           JOIN "topics" t ON t."topic_id" = r."topic_id"
           WHERE r."reply_ref" = ?1
             AND ((t."author_id" = ?2 AND t."forum_id" = ?3)
                  OR (t."author_id" = ?3 AND t."forum_id" = ?2))"#,
        reply_ref,
        one,
        other,
    )
    .fetch_optional(&mut *tx)
    .await?)
}

/// A new conversation, begun by a message from `sender` with `subject` and
/// `body` sent at `sent`, in `recipient`'s inbox.
async fn start_conversation(
    tx: &mut Immediate,
    sender: &str,
    recipient: &str,
    subject: &str,
    body: &str,
    sent: &str,
) -> Result<String> {
    ensure_personal_forum(&mut *tx, recipient).await?;

    let topic = Ulid::new().to_string();
    sqlx::query!(
        r#"INSERT INTO "topics" ("topic_id", "forum_id", "title", "content", "author_id", "created_at", "updated_at", "is_pinned", "is_locked", "is_deleted", "is_flagged")
           VALUES (?, ?, ?, ?, ?, ?, ?, 0, 0, 0, 0)"#,
        topic,
        recipient,
        subject,
        body,
        sender,
        sent,
        sent,
    )
    .execute(&mut *tx)
    .await?;

    Ok(topic)
}

/// Bring `message`, from `sender` to `recipient` and now in `topic`,
/// together with the rest of its thread between the same two: fold its
/// conversation into that of the message it answers, and fold those of the
/// answers to it copied before it into its own. The conversation the thread
/// is in.
async fn gather(
    tx: &mut Immediate,
    message: &OrreryMessage,
    sender: &str,
    recipient: &str,
    topic: String,
) -> Result<String> {
    let mut thread = topic;
    if let Some(parent) = &message.in_reply_to_reply_ref
        && let Some(parent_topic) = conversation_of(tx, parent, sender, recipient).await?
        && parent_topic != thread
    {
        fold(tx, &thread, &parent_topic).await?;
        thread = parent_topic;
    }

    let answers = sqlx::query_scalar!(
        r#"SELECT DISTINCT p."topic_id" FROM "orrery_messages" om
           JOIN "posts" p ON p."post_id" = om."post_id"
           WHERE om."parent_reply_ref" = ?1 AND p."topic_id" <> ?2
             AND ((om."sender_id" = ?3 AND om."recipient_id" = ?4)
                  OR (om."sender_id" = ?4 AND om."recipient_id" = ?3))"#,
        message.reply_ref,
        thread,
        sender,
        recipient,
    )
    .fetch_all(&mut *tx)
    .await?;
    for answers_topic in answers {
        fold(tx, &answers_topic, &thread).await?;
    }

    Ok(thread)
}

/// Move everything in the conversation `from` into `into`, and remove
/// `from`.
async fn fold(tx: &mut Immediate, from: &str, into: &str) -> Result<()> {
    sqlx::query!(
        r#"UPDATE "posts" SET "topic_id" = ? WHERE "topic_id" = ?"#,
        into,
        from
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        r#"UPDATE "message_reply_refs" SET "topic_id" = ? WHERE "topic_id" = ?"#,
        into,
        from
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        r#"UPDATE "topics" SET "updated_at" = (
               SELECT MAX(COALESCE(t."updated_at", t."created_at")) FROM "topics" t
               WHERE t."topic_id" IN (?, ?))
           WHERE "topic_id" = ?"#,
        from,
        into,
        into,
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(r#"DELETE FROM "message_status" WHERE "topic_id" = ?"#, from)
        .execute(&mut *tx)
        .await?;

    sqlx::query!(r#"DELETE FROM "topic_tags" WHERE "topic_id" = ?"#, from)
        .execute(&mut *tx)
        .await?;

    sqlx::query!(r#"DELETE FROM "topics" WHERE "topic_id" = ?"#, from)
        .execute(&mut *tx)
        .await?;

    Ok(())
}

/// Move each copy placed in a conversation between another two than its
/// sender and recipient, as copies were before conversations kept to two,
/// into the conversation of the message it answers if that one is between
/// the same two, or else a new one in its recipient's inbox, under the title
/// of the conversation it leaves. The copies are moved in the order they
/// were sent, so that an answer finds its message already moved.
async fn separate_pairs(pool: &SqlitePool) -> Result<()> {
    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    let misplaced = sqlx::query!(
        r#"SELECT om."post_id", om."reply_ref", om."parent_reply_ref", om."sender_id",
                  om."recipient_id", om."sent", p."content", p."topic_id" AS "topic_id!: String",
                  t."title"
           FROM "orrery_messages" om
           JOIN "posts" p ON p."post_id" = om."post_id"
           JOIN "topics" t ON t."topic_id" = p."topic_id"
           WHERE NOT ((t."author_id" = om."sender_id" AND t."forum_id" = om."recipient_id")
                      OR (t."author_id" = om."recipient_id" AND t."forum_id" = om."sender_id"))
           ORDER BY om."sent", om."orrery_id""#
    )
    .fetch_all(&mut tx)
    .await?;

    let mut topics = BTreeSet::new();
    for copy in misplaced {
        let parent_topic = match &copy.parent_reply_ref {
            Some(parent) => {
                conversation_of(&mut tx, parent, &copy.sender_id, &copy.recipient_id).await?
            }
            None => None,
        };
        let topic = match parent_topic {
            Some(topic) => topic,
            None => {
                start_conversation(
                    &mut tx,
                    &copy.sender_id,
                    &copy.recipient_id,
                    &copy.title,
                    &copy.content,
                    &copy.sent,
                )
                .await?
            }
        };

        sqlx::query!(
            r#"UPDATE "posts" SET "topic_id" = ? WHERE "post_id" = ?"#,
            topic,
            copy.post_id
        )
        .execute(&mut tx)
        .await?;
        sqlx::query!(
            r#"UPDATE "message_reply_refs" SET "topic_id" = ? WHERE "reply_ref" = ?"#,
            topic,
            copy.reply_ref
        )
        .execute(&mut tx)
        .await?;

        topics.insert(copy.topic_id);
        topics.insert(topic);
    }

    for topic in topics {
        sqlx::query!(
            r#"UPDATE "topics" SET "updated_at" = (
                   SELECT MAX(p."created_at") FROM "posts" p WHERE p."topic_id" = ?1)
               WHERE "topic_id" = ?1"#,
            topic
        )
        .execute(&mut tx)
        .await?;
        update_statuses(&mut tx, &topic).await?;
    }

    tx.commit().await?;

    Ok(())
}

/// Set where each participant of the conversation `topic` stands with it
/// from the last message there they can see, as orrery says it: whoever
/// sent that message has seen it and kept it; whoever it went to has seen
/// or discarded it as orrery says. A participant with no message there to
/// see has no standing kept, nor has anyone who is not one of the two
/// participants: one whose message was moved out of the conversation.
async fn update_statuses(tx: &mut Immediate, topic: &str) -> Result<()> {
    sqlx::query!(
        r#"DELETE FROM "message_status"
           WHERE "topic_id" = ?1
             AND "user_id" NOT IN (SELECT "author_id" FROM "topics" WHERE "topic_id" = ?1
                                   UNION ALL
                                   SELECT "forum_id" FROM "topics" WHERE "topic_id" = ?1)"#,
        topic
    )
    .execute(&mut *tx)
    .await?;

    let participants = sqlx::query_scalar!(
        r#"SELECT om."sender_id" AS "user_id!: String" FROM "orrery_messages" om
           JOIN "posts" p ON p."post_id" = om."post_id" WHERE p."topic_id" = ?1
           UNION
           SELECT om."recipient_id" FROM "orrery_messages" om
           JOIN "posts" p ON p."post_id" = om."post_id" WHERE p."topic_id" = ?1"#,
        topic
    )
    .fetch_all(&mut *tx)
    .await?;

    for user in participants {
        let last = sqlx::query!(
            r#"SELECT om."sender_id", om."seen" AS "seen: bool", om."discarded" AS "discarded: bool"
               FROM "orrery_messages" om
               JOIN "posts" p ON p."post_id" = om."post_id"
               WHERE p."topic_id" = ?1 AND p."is_deleted" = 0
                 AND (p."approved" = 1 OR om."sender_id" = ?2)
                 AND (om."sender_id" = ?2 OR om."recipient_id" = ?2)
               ORDER BY om."sent" DESC, om."orrery_id" DESC
               LIMIT 1"#,
            topic,
            user,
        )
        .fetch_optional(&mut *tx)
        .await?;

        match last {
            Some(last) => {
                let (seen, discarded) = if last.sender_id == user {
                    (true, false)
                } else {
                    (last.seen, last.discarded)
                };
                sqlx::query!(
                    r#"INSERT INTO "message_status" ("user_id", "topic_id", "seen", "discarded")
                       VALUES (?, ?, ?, ?)
                       ON CONFLICT ("user_id", "topic_id") DO UPDATE SET
                           "seen" = excluded."seen", "discarded" = excluded."discarded""#,
                    user,
                    topic,
                    seen,
                    discarded,
                )
                .execute(&mut *tx)
                .await?;
            }
            None => {
                sqlx::query!(
                    r#"DELETE FROM "message_status" WHERE "user_id" = ? AND "topic_id" = ?"#,
                    user,
                    topic,
                )
                .execute(&mut *tx)
                .await?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    const ANN: &str = "01HZZZZZZZZZZZZZZZZZZZZANN";
    const BOB: &str = "01HZZZZZZZZZZZZZZZZZZZZBOB";
    const CAL: &str = "01HZZZZZZZZZZZZZZZZZZZZCAL";

    async fn migrated() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        pool
    }

    /// Orrery's message `id`, from `from` to `to`, sent `id` minutes after ten,
    /// answering `parent` (a reply reference) if given.
    fn message(id: i32, from: &str, to: &str, parent: Option<&str>) -> OrreryMessage {
        OrreryMessage {
            id,
            from_ulid: Some(from.to_string()),
            to_ulid: Some(to.to_string()),
            subject: format!("Subject {id}"),
            body: format!("Body {id}"),
            sent: format!("2026-11-18T{:02}:{:02}:00Z", 10 + id / 60, id % 60),
            seen: false,
            discarded: false,
            in_reply_to_reply_ref: parent.map(String::from),
            reply_ref: format!("ref-{id}"),
            review_status: ReviewStatus::Approved,
            changed: None,
        }
    }

    /// The conversation the copy of orrery's message `id` is in.
    async fn topic_of_copy(pool: &SqlitePool, id: i32) -> String {
        sqlx::query_scalar(
            r#"SELECT p."topic_id" FROM "orrery_messages" om
               JOIN "posts" p ON p."post_id" = om."post_id" WHERE om."orrery_id" = ?"#,
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn count(pool: &SqlitePool, table: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            r#"SELECT COUNT(*) FROM "{table}""#
        )))
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// `user`'s (seen, discarded) for `topic`, if any.
    async fn status(pool: &SqlitePool, user: &str, topic: &str) -> Option<(bool, bool)> {
        sqlx::query_as(
            r#"SELECT "seen", "discarded" FROM "message_status" WHERE "user_id" = ? AND "topic_id" = ?"#,
        )
        .bind(user)
        .bind(topic)
        .fetch_optional(pool)
        .await
        .unwrap()
    }

    /// The (approved, is_deleted) of the copy of orrery's message `id`.
    async fn post_state(pool: &SqlitePool, id: i32) -> (bool, bool) {
        sqlx::query_as(
            r#"SELECT p."approved", p."is_deleted" FROM "orrery_messages" om
               JOIN "posts" p ON p."post_id" = om."post_id" WHERE om."orrery_id" = ?"#,
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[test]
    fn a_feed_that_doesnt_say_sends_delivered_messages() {
        let old: OrreryMessage = serde_json::from_str(
            r#"{"id": 1, "from_ulid": null, "to_ulid": null, "subject": "", "body": "",
                "sent": "2026-11-18T10:00:00Z", "seen": false, "discarded": false,
                "group": null, "in_reply_to_reply_ref": null, "reply_ref": "r"}"#,
        )
        .unwrap();
        check!(old.review_status == ReviewStatus::Approved);
        check!(old.changed.is_none());

        let new: OrreryMessage = serde_json::from_str(
            r#"{"id": 1, "from_ulid": null, "to_ulid": null, "subject": "", "body": "",
                "sent": "2026-11-18T10:00:00Z", "seen": false, "discarded": false,
                "group": null, "in_reply_to_reply_ref": null, "reply_ref": "r",
                "review_status": "denied", "changed": "2026-11-18T10:00:00.123456Z"}"#,
        )
        .unwrap();
        check!(new.review_status == ReviewStatus::Denied);
        check!(new.changed.unwrap().parse::<jiff::Timestamp>().is_ok());
    }

    #[tokio::test]
    async fn a_message_opens_a_conversation_in_its_recipients_inbox() {
        let pool = migrated().await;
        import(&pool, &message(1, ANN, BOB, None)).await.unwrap();

        let topic = topic_of_copy(&pool, 1).await;
        let forum: String =
            sqlx::query_scalar(r#"SELECT "forum_id" FROM "topics" WHERE "topic_id" = ?"#)
                .bind(&topic)
                .fetch_one(&pool)
                .await
                .unwrap();
        check!(forum == BOB);
        check!(post_state(&pool, 1).await == (true, false));
        check!(status(&pool, ANN, &topic).await == Some((true, false)));
        check!(status(&pool, BOB, &topic).await == Some((false, false)));
    }

    #[tokio::test]
    async fn copying_a_message_again_brings_its_copy_up_to_date() {
        let pool = migrated().await;
        let mut opener = message(1, ANN, BOB, None);
        import(&pool, &opener).await.unwrap();

        opener.seen = true;
        opener.discarded = true;
        import(&pool, &opener).await.unwrap();
        check!(count(&pool, "posts").await == 1);
        check!(count(&pool, "topics").await == 1);
        let topic = topic_of_copy(&pool, 1).await;
        check!(status(&pool, BOB, &topic).await == Some((true, true)));

        opener.review_status = ReviewStatus::Pending;
        import(&pool, &opener).await.unwrap();
        check!(post_state(&pool, 1).await == (false, false));
        // A held message is the sender's alone to see.
        check!(status(&pool, BOB, &topic).await.is_none());
        check!(status(&pool, ANN, &topic).await == Some((true, false)));

        opener.review_status = ReviewStatus::Denied;
        import(&pool, &opener).await.unwrap();
        check!(post_state(&pool, 1).await == (false, true));
        check!(status(&pool, ANN, &topic).await.is_none());
    }

    #[tokio::test]
    async fn a_message_without_both_participants_is_passed_over() {
        let pool = migrated().await;
        let mut orphan = message(1, ANN, BOB, None);
        orphan.to_ulid = None;
        import(&pool, &orphan).await.unwrap();
        check!(count(&pool, "posts").await == 0);
    }

    #[tokio::test]
    async fn a_thread_is_one_conversation_whatever_order_it_comes_in() {
        // 1 opens; 2 answers 1; 3 answers 2; 4 answers 3.
        let thread = [
            message(1, ANN, BOB, None),
            message(2, BOB, ANN, Some("ref-1")),
            message(3, ANN, BOB, Some("ref-2")),
            message(4, BOB, ANN, Some("ref-3")),
        ];
        for order in [[0, 1, 2, 3], [3, 2, 1, 0], [2, 0, 3, 1], [1, 3, 0, 2]] {
            let pool = migrated().await;
            for i in order {
                import(&pool, &thread[i]).await.unwrap();
            }

            check!(count(&pool, "topics").await == 1, "{order:?}");
            check!(count(&pool, "posts").await == 4, "{order:?}");
            let topic = topic_of_copy(&pool, 1).await;
            for id in 2..=4 {
                check!(topic_of_copy(&pool, id).await == topic, "{order:?}");
            }
            let (title, forum, updated): (String, String, String) = sqlx::query_as(
                r#"SELECT "title", "forum_id", "updated_at" FROM "topics" WHERE "topic_id" = ?"#,
            )
            .bind(&topic)
            .fetch_one(&pool)
            .await
            .unwrap();
            check!(title == "Subject 1", "{order:?}");
            check!(forum == BOB, "{order:?}");
            check!(updated == "2026-11-18T10:04:00Z", "{order:?}");
            // The last message, 4, went from Bob to Ann.
            check!(
                status(&pool, BOB, &topic).await == Some((true, false)),
                "{order:?}"
            );
            check!(
                status(&pool, ANN, &topic).await == Some((false, false)),
                "{order:?}"
            );
            check!(count(&pool, "message_status").await == 2, "{order:?}");
        }
    }

    #[tokio::test]
    async fn copies_made_before_copies_were_linked_are_claimed() {
        let pool = migrated().await;
        // As the importer copied a thread of three before it linked copies:
        // the opener's reference kept; the answer to it put in its
        // conversation and its reference dropped; the answer to that, whose
        // message it couldn't find, put in a conversation of its own.
        for (topic, forum, author, id) in [("T1", BOB, ANN, 1), ("T3", BOB, ANN, 3)] {
            let m = message(id, author, forum, None);
            sqlx::query(
                r#"INSERT INTO "forums" ("forum_id", "slug", "name", "description", "created_at", "is_active", "display_order")
                   VALUES (?1, 'inbox-' || ?1, 'Inbox', '', '2026-11-17T00:00:00Z', 1, 0)
                   ON CONFLICT DO NOTHING"#,
            )
            .bind(forum)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                r#"INSERT INTO "topics" ("topic_id", "forum_id", "title", "content", "author_id", "created_at", "updated_at", "is_pinned", "is_locked", "is_deleted", "is_flagged")
                   VALUES (?, ?, ?, ?, ?, ?, ?, 0, 0, 0, 0)"#,
            )
            .bind(topic)
            .bind(forum)
            .bind(&m.subject)
            .bind(&m.body)
            .bind(author)
            .bind(&m.sent)
            .bind(&m.sent)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                r#"INSERT INTO "message_reply_refs" ("reply_ref", "topic_id") VALUES (?, ?)"#,
            )
            .bind(&m.reply_ref)
            .bind(topic)
            .execute(&pool)
            .await
            .unwrap();
        }
        for (post, topic, m) in [
            ("P1", "T1", message(1, ANN, BOB, None)),
            ("P2", "T1", message(2, BOB, ANN, Some("ref-1"))),
            ("P3", "T3", message(3, ANN, BOB, Some("ref-2"))),
        ] {
            sqlx::query(
                r#"INSERT INTO "posts" ("post_id", "topic_id", "content", "author_id", "created_at", "is_deleted", "is_flagged", "approved")
                   VALUES (?, ?, ?, ?, ?, 0, 0, 1)"#,
            )
            .bind(post)
            .bind(topic)
            .bind(&m.body)
            .bind(m.from_ulid.as_deref())
            .bind(&m.sent)
            .execute(&pool)
            .await
            .unwrap();
        }

        for m in [
            message(1, ANN, BOB, None),
            message(2, BOB, ANN, Some("ref-1")),
            message(3, ANN, BOB, Some("ref-2")),
        ] {
            import(&pool, &m).await.unwrap();
        }

        check!(count(&pool, "posts").await == 3);
        check!(count(&pool, "topics").await == 1);
        check!(count(&pool, "message_reply_refs").await == 3);
        let claimed: Vec<String> =
            sqlx::query_scalar(r#"SELECT "post_id" FROM "orrery_messages" ORDER BY "orrery_id""#)
                .fetch_all(&pool)
                .await
                .unwrap();
        check!(claimed == ["P1", "P2", "P3"]);
        check!(topic_of_copy(&pool, 3).await == "T1");
    }

    #[tokio::test]
    async fn the_copies_no_message_claims_are_deleted() {
        let pool = migrated().await;
        for id in 1..=150 {
            import(&pool, &message(id, ANN, BOB, None)).await.unwrap();
        }
        // An unclaimed copy in a copied conversation, and a conversation of
        // hydra's own.
        let copied_topic = topic_of_copy(&pool, 7).await;
        sqlx::query(
            r#"INSERT INTO "topics" ("topic_id", "forum_id", "title", "content", "author_id", "created_at", "updated_at", "is_pinned", "is_locked", "is_deleted", "is_flagged")
               VALUES ('HYDRA', ?, 'Hi', 'Hi', ?, '2026-11-18T09:00:00Z', '2026-11-18T09:00:00Z', 0, 0, 0, 0)"#,
        )
        .bind(BOB)
        .bind(ANN)
        .execute(&pool)
        .await
        .unwrap();
        for (post, topic) in [("GONE", copied_topic.as_str()), ("OWN", "HYDRA")] {
            sqlx::query(
                r#"INSERT INTO "posts" ("post_id", "topic_id", "content", "author_id", "created_at", "is_deleted", "is_flagged", "approved")
                   VALUES (?, ?, 'Gone', ?, '2026-11-18T09:30:00Z', 0, 0, 1)"#,
            )
            .bind(post)
            .bind(topic)
            .bind(BOB)
            .execute(&pool)
            .await
            .unwrap();
        }

        delete_unclaimed(&pool).await.unwrap();

        let deleted: Vec<String> =
            sqlx::query_scalar(r#"SELECT "post_id" FROM "posts" WHERE "is_deleted" = 1"#)
                .fetch_all(&pool)
                .await
                .unwrap();
        check!(deleted == ["GONE"]);
        check!(live_copies(&pool).await.unwrap().len() == 150);
    }

    #[tokio::test]
    async fn so_many_unclaimed_copies_are_refused() {
        let pool = migrated().await;
        import(&pool, &message(1, ANN, BOB, None)).await.unwrap();
        sqlx::query(r#"DELETE FROM "orrery_messages""#)
            .execute(&pool)
            .await
            .unwrap();

        check!(delete_unclaimed(&pool).await.is_err());
        check!(post_state_of(&pool).await == (true, false));
    }

    /// The (approved, is_deleted) of the only post.
    async fn post_state_of(pool: &SqlitePool) -> (bool, bool) {
        sqlx::query_as(r#"SELECT "approved", "is_deleted" FROM "posts""#)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn the_sweep_deletes_the_copies_orrery_no_longer_has() {
        let pool = migrated().await;
        for id in 1..=150 {
            import(&pool, &message(id, ANN, BOB, None)).await.unwrap();
        }
        let copied = live_copies(&pool).await.unwrap();
        check!(copied.len() == 150);

        // A list that would take more than one copy in a hundred is refused.
        let present: HashSet<i32> = (3..=150).collect();
        check!(delete_absent(&pool, &copied, &present).await.is_err());
        check!(live_copies(&pool).await.unwrap().len() == 150);

        let present: HashSet<i32> = (1..=150).filter(|&id| id != 42).collect();
        delete_absent(&pool, &copied, &present).await.unwrap();
        check!(post_state(&pool, 42).await == (true, true));
        check!(live_copies(&pool).await.unwrap().len() == 149);
        let topic = topic_of_copy(&pool, 42).await;
        check!(status(&pool, BOB, &topic).await.is_none());
    }

    /// The (forum, author, title) of the conversation `topic`.
    async fn conversation(pool: &SqlitePool, topic: &str) -> (String, String, String) {
        sqlx::query_as(
            r#"SELECT "forum_id", "author_id", "title" FROM "topics" WHERE "topic_id" = ?"#,
        )
        .bind(topic)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn an_answer_between_another_two_goes_in_a_conversation_of_theirs() {
        // 1 opens, from Ann to Bob; 2 answers 1, but from Bob to Cal; 3
        // answers 2, from Cal to Bob.
        let thread = [
            message(1, ANN, BOB, None),
            message(2, BOB, CAL, Some("ref-1")),
            message(3, CAL, BOB, Some("ref-2")),
        ];
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let pool = migrated().await;
            for i in order {
                import(&pool, &thread[i]).await.unwrap();
            }

            check!(count(&pool, "topics").await == 2, "{order:?}");
            let first = topic_of_copy(&pool, 1).await;
            let second = topic_of_copy(&pool, 2).await;
            check!(first != second, "{order:?}");
            check!(topic_of_copy(&pool, 3).await == second, "{order:?}");
            check!(conversation(&pool, &first).await.0 == BOB, "{order:?}");
            let (forum, author, _) = conversation(&pool, &second).await;
            check!((forum.as_str(), author.as_str()) == (CAL, BOB), "{order:?}");
            check!(status(&pool, CAL, &first).await.is_none(), "{order:?}");
        }
    }

    #[tokio::test]
    async fn copies_in_a_conversation_between_another_two_are_separated() {
        let pool = migrated().await;
        for m in [
            message(1, ANN, BOB, None),
            message(2, BOB, CAL, Some("ref-1")),
            message(3, CAL, BOB, Some("ref-2")),
        ] {
            import(&pool, &m).await.unwrap();
        }
        // As the importer placed them when a thread was one conversation:
        // all three in the one the first opened.
        let first = topic_of_copy(&pool, 1).await;
        let second = topic_of_copy(&pool, 2).await;
        for table in ["posts", "message_reply_refs"] {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                r#"UPDATE "{table}" SET "topic_id" = ?"#
            )))
            .bind(&first)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query(r#"DELETE FROM "topics" WHERE "topic_id" = ?"#)
            .bind(&second)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(r#"DELETE FROM "message_status" WHERE "topic_id" = ?"#)
            .bind(&second)
            .execute(&pool)
            .await
            .unwrap();
        update_statuses_of(&pool, &first).await;
        check!(status(&pool, CAL, &first).await.is_some());

        separate_pairs(&pool).await.unwrap();

        check!(count(&pool, "topics").await == 2);
        check!(topic_of_copy(&pool, 1).await == first);
        let second = topic_of_copy(&pool, 2).await;
        check!(second != first);
        check!(topic_of_copy(&pool, 3).await == second);
        check!(
            conversation(&pool, &second).await
                == (CAL.to_string(), BOB.to_string(), "Subject 1".to_string())
        );
        check!(status(&pool, CAL, &first).await.is_none());
        check!(status(&pool, BOB, &second).await == Some((false, false)));
        check!(status(&pool, CAL, &second).await == Some((true, false)));
        let updated: (String, String) = sqlx::query_as(
            r#"SELECT a."updated_at", b."updated_at" FROM "topics" a, "topics" b
               WHERE a."topic_id" = ? AND b."topic_id" = ?"#,
        )
        .bind(&first)
        .bind(&second)
        .fetch_one(&pool)
        .await
        .unwrap();
        check!(
            updated
                == (
                    "2026-11-18T10:01:00Z".to_string(),
                    "2026-11-18T10:03:00Z".to_string()
                )
        );

        // Separating them again changes nothing.
        separate_pairs(&pool).await.unwrap();
        check!(count(&pool, "topics").await == 2);
    }

    async fn update_statuses_of(pool: &SqlitePool, topic: &str) {
        let mut tx = Immediate::begin_with_retry(pool, None, None).await.unwrap();
        update_statuses(&mut tx, topic).await.unwrap();
        tx.commit().await.unwrap();
    }
}
