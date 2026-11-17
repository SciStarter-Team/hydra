//! The copy of orrery's message templates kept until cutover, as one-offs.
//!
//! Templates come a page at a time, in the order orrery last changed them
//! (`/staff/sync/message_templates`). Orrery stamps that change when it
//! writes a row, not when the write is committed, so a row can appear behind
//! the cursor; each round reads again from a few minutes behind where the
//! last ended, and copying a template twice changes nothing. Deletions are
//! found by comparing every id orrery still has
//! (`/staff/sync/message_template_ids`), and archive the copy rather than
//! delete it.
//!
//! A copy is keyed on orrery's id for its template, which a rename leaves
//! alone. Copies made before they carried it are claimed by slug, the first
//! time orrery sends their template; those no template claims, by the end of
//! the first round, are of templates orrery has since renamed or deleted.
//!
//! The sync writes a copy's subject, body, slug, reply-to and unsubscribe
//! setting, which are orrery's (see `OneoffEmail::kept_by_orrery`), and
//! leaves its data sets alone, which are hydra's.

use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Error, anyhow};
use common::{
    misc, primary::CopyKeeper, shared::db, timestring::TimeString, transaction::Immediate,
};
use regex::Regex;
use serde::Deserialize;
use sqlx::SqlitePool;
use tokio::time::sleep;
use ulid::Ulid;

/// How far behind its cursor each round reads templates again.
const OVERLAP: jiff::SignedDuration = jiff::SignedDuration::from_mins(5);

/// Orrery's page of templates holds at most this many.
const PAGE: usize = 500;

const CURSOR_CHANGED: &str = "template-sync-changed";
const CURSOR_ID: &str = "template-sync-id";

/// Set once a round has read every template orrery has, and so claimed
/// every copy that has a template to claim it.
const CLAIMED: &str = "template-sync-claimed";

/// One of orrery's message templates, as `/staff/sync/message_templates`
/// sends it.
#[derive(Debug, Clone, Deserialize)]
struct OrreryTemplate {
    id: i64,
    slug: String,
    subject: String,
    body: String,
    reply_to_noreply: bool,
    no_unsubscribe_link: bool,
    changed: String,
}

/// Copy orrery's message templates, until hydra keeps its own.
pub async fn synchronize() {
    if !CopyKeeper::NOW.orrery_keeps_copies() {
        return;
    }

    loop {
        let pause = match round().await {
            Ok(()) => Duration::from_secs(15 * 60),
            Err(e) => {
                eprintln!("Template sync error: {e}");
                Duration::from_secs(60)
            }
        };

        sleep(pause).await;
    }
}

/// Copy every template changed since the cursor, then archive the copies of
/// those orrery no longer has.
///
/// A template that cannot be copied ends the round there, with the cursor
/// left before it, so that it is tried again rather than passed over.
async fn round() -> Result<(), Error> {
    let client = reqwest::Client::new();

    let (changed, _) = cursor().await?;
    let mut since = changed
        .map(|changed| changed - OVERLAP)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    let mut last_id = 0;

    loop {
        let page: Vec<OrreryTemplate> = client
            .get("https://scistarter.org/staff/sync/message_templates")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&[
                (
                    "since",
                    since.strftime("%Y-%m-%dT%H:%M:%S%.f+00:00").to_string(),
                ),
                ("last_id", last_id.to_string()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        for template in &page {
            mirror(db(), template)
                .await
                .map_err(|e| anyhow!("template {} ({}): {e}", template.id, template.slug))?;

            since = template.changed.parse()?;
            last_id = template.id;
            advance_cursor(since, last_id).await?;
        }

        if page.len() < PAGE {
            break;
        }
    }

    if misc::get::<bool>(CLAIMED).await?.is_none() {
        archive_unclaimed(db()).await?;
        misc::set(CLAIMED, true).await?;
    }

    let ids: Vec<i64> = client
        .get("https://scistarter.org/staff/sync/message_template_ids")
        .bearer_auth(env!("SYNC_TOKEN"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    archive_deleted(db(), &ids).await?;

    Ok(())
}

/// Where the last round stopped: the time orrery last changed the last
/// template it copied, and that template's id.
async fn cursor() -> Result<(Option<jiff::Timestamp>, i64), Error> {
    let changed: Option<String> = misc::get(CURSOR_CHANGED).await?;
    let id: Option<i64> = misc::get(CURSOR_ID).await?;

    Ok((
        changed.map(|changed| changed.parse()).transpose()?,
        id.unwrap_or(0),
    ))
}

/// Move the cursor on to `changed` and `id`, unless it is past them already,
/// as it is while a round reads again behind it.
async fn advance_cursor(changed: jiff::Timestamp, id: i64) -> Result<(), Error> {
    let (kept_changed, kept_id) = cursor().await?;

    if kept_changed.is_some_and(|kept| (kept, kept_id) >= (changed, id)) {
        return Ok(());
    }

    misc::set(CURSOR_CHANGED, changed.to_string()).await?;
    misc::set(CURSOR_ID, id).await?;

    Ok(())
}

static LEGACY_MERGE_FIELD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{([a-zA-Z_][a-zA-Z0-9_]*)\}").expect("Invalid regex"));

/// Orrery's merge fields, `{fieldname}`, written as hydra's,
/// `{{FIELDNAME}}`.
fn convert_merge_syntax(html: &str) -> String {
    LEGACY_MERGE_FIELD_RE
        .replace_all(html, |caps: &regex::Captures| {
            format!("{{{{{}}}}}", caps[1].to_uppercase())
        })
        .into_owned()
}

/// Make the copy of `template` what orrery has: claiming the copy made
/// before copies carried orrery's id, if there is one, or making one if
/// there is none.
async fn mirror(pool: &SqlitePool, template: &OrreryTemplate) -> Result<(), Error> {
    // The subject is merged at send time from the same fields as the body,
    // so it is converted too: left raw, a subject like "Regarding Your
    // SciStarter Submission: {project_name}" never matches, and goes out
    // with the brace still in it.
    let subject = convert_merge_syntax(&template.subject);
    let body_delta = common::quill::html_to_delta(&convert_merge_syntax(&template.body))?;

    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    // A copy orrery's template has not claimed yet, under its slug. It is
    // brought back from the archive if it was put there as unclaimed: it
    // was only waiting for its template to come round.
    sqlx::query!(
        r#"UPDATE oneoff_emails SET orrery_id = ?1, archived = 0
           WHERE oneoff_email_id = (
                     SELECT oneoff_email_id FROM oneoff_emails
                     WHERE orrery_id IS NULL AND legacy_slug = ?2
                     ORDER BY created_at LIMIT 1)
             AND NOT EXISTS (SELECT 1 FROM oneoff_emails WHERE orrery_id = ?1)"#,
        template.id,
        template.slug,
    )
    .execute(&mut tx)
    .await?;

    let updated = sqlx::query!(
        r#"UPDATE oneoff_emails
           SET subject = ?, body_delta = ?, legacy_slug = ?, reply_to_noreply = ?,
               no_unsubscribe = ?
           WHERE orrery_id = ?"#,
        subject,
        body_delta,
        template.slug,
        template.reply_to_noreply,
        template.no_unsubscribe_link,
        template.id,
    )
    .execute(&mut tx)
    .await?
    .rows_affected();

    if updated == 0 {
        let oneoff_email_id = Ulid::new().to_string();
        let now = TimeString::now().to_string();

        sqlx::query!(
            r#"INSERT INTO oneoff_emails
                   (oneoff_email_id, subject, body_delta, datasets, created_at, archived,
                    legacy_slug, reply_to_noreply, no_unsubscribe, orrery_id)
               VALUES (?, ?, ?, '[]', ?, 0, ?, ?, ?, ?)"#,
            oneoff_email_id,
            subject,
            body_delta,
            now,
            template.slug,
            template.reply_to_noreply,
            template.no_unsubscribe_link,
            template.id,
        )
        .execute(&mut tx)
        .await?;
    }

    tx.commit().await?;

    Ok(())
}

/// Archive the copies no template claimed in a round that read every
/// template orrery has: copies of templates it has since renamed or
/// deleted.
async fn archive_unclaimed(pool: &SqlitePool) -> Result<(), Error> {
    sqlx::query!(
        r#"UPDATE oneoff_emails SET archived = 1
           WHERE orrery_id IS NULL AND legacy_slug IS NOT NULL"#
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Archive the copies of the templates orrery no longer has: those whose
/// ids are not among `ids`, every id it has. Archived rather than deleted,
/// so that what was sent from them still says what it was.
///
/// Orrery deletes a template now and then, by hand; a list that would take
/// most of the copies with it is a fault on its side, and is refused.
async fn archive_deleted(pool: &SqlitePool, ids: &[i64]) -> Result<(), Error> {
    if ids.is_empty() {
        return Err(anyhow!("orrery sent no template ids"));
    }

    let ids = serde_json::to_string(ids)?;
    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    let counts = sqlx::query!(
        r#"SELECT COUNT(*) AS "copies!: i64",
                  COUNT(*) FILTER (WHERE orrery_id NOT IN (SELECT value FROM json_each(?)))
                      AS "deleted!: i64"
           FROM oneoff_emails
           WHERE orrery_id IS NOT NULL AND archived = 0"#,
        ids
    )
    .fetch_one(&mut tx)
    .await?;

    if counts.deleted * 4 > counts.copies {
        return Err(anyhow!(
            "orrery's template ids would archive {} of {} copies",
            counts.deleted,
            counts.copies
        ));
    }

    sqlx::query!(
        r#"UPDATE oneoff_emails SET archived = 1
           WHERE orrery_id IS NOT NULL AND archived = 0
             AND orrery_id NOT IN (SELECT value FROM json_each(?))"#,
        ids
    )
    .execute(&mut tx)
    .await?;

    tx.commit().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::migrated;
    use assert2::check;

    fn template(id: i64, slug: &str, subject: &str) -> OrreryTemplate {
        OrreryTemplate {
            id,
            slug: slug.to_string(),
            subject: subject.to_string(),
            body: "<p>Hello {first_name}</p>".to_string(),
            reply_to_noreply: false,
            no_unsubscribe_link: false,
            changed: "2026-11-17T17:06:12.345678+00:00".to_string(),
        }
    }

    /// A copy made before copies carried orrery's id, as the sync made them.
    async fn legacy_copy(pool: &SqlitePool, slug: &str, datasets: &str) -> String {
        let id = Ulid::new().to_string();
        sqlx::query(
            r#"INSERT INTO oneoff_emails
                   (oneoff_email_id, subject, body_delta, datasets, created_at, archived,
                    legacy_slug)
               VALUES (?, 'Old', '{"ops":[]}', ?, '2026-03-23T14:35:56Z', 0, ?)"#,
        )
        .bind(&id)
        .bind(datasets)
        .bind(slug)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    #[derive(Debug, sqlx::FromRow)]
    struct Copy {
        oneoff_email_id: String,
        subject: String,
        body_delta: String,
        datasets: String,
        legacy_slug: Option<String>,
        archived: bool,
        no_unsubscribe: bool,
    }

    async fn copy_of(pool: &SqlitePool, orrery_id: i64) -> Option<Copy> {
        sqlx::query_as(
            r#"SELECT oneoff_email_id, subject, body_delta, datasets, legacy_slug, archived,
                      no_unsubscribe
               FROM oneoff_emails WHERE orrery_id = ?"#,
        )
        .bind(orrery_id)
        .fetch_optional(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_new_template_is_copied_with_hydras_merge_fields() {
        let pool = migrated().await;
        let mut new = template(7, "welcome", "Hi {first_name}");
        new.no_unsubscribe_link = true;

        mirror(&pool, &new).await.unwrap();

        let copy = copy_of(&pool, 7).await.unwrap();
        check!(copy.subject == "Hi {{FIRST_NAME}}");
        check!(copy.body_delta.contains("{{FIRST_NAME}}"));
        check!(copy.legacy_slug.as_deref() == Some("welcome"));
        check!(copy.no_unsubscribe);
    }

    #[tokio::test]
    async fn an_edit_or_rename_rewrites_the_copy_and_keeps_its_data_sets() {
        let pool = migrated().await;
        let kept = legacy_copy(&pool, "welcome", r#"["user"]"#).await;

        mirror(&pool, &template(7, "welcome", "First draft"))
            .await
            .unwrap();
        mirror(&pool, &template(7, "welcome-v2", "Second draft"))
            .await
            .unwrap();

        let copy = copy_of(&pool, 7).await.unwrap();
        check!(copy.oneoff_email_id == kept);
        check!(copy.subject == "Second draft");
        check!(copy.legacy_slug.as_deref() == Some("welcome-v2"));
        check!(copy.datasets == r#"["user"]"#);

        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM oneoff_emails")
            .fetch_one(&pool)
            .await
            .unwrap();
        check!(rows == 1);
    }

    #[tokio::test]
    async fn copies_no_template_claims_are_archived_until_one_does() {
        let pool = migrated().await;
        legacy_copy(&pool, "renamed-away", "[]").await;

        archive_unclaimed(&pool).await.unwrap();
        let archived: bool = sqlx::query_scalar("SELECT archived FROM oneoff_emails")
            .fetch_one(&pool)
            .await
            .unwrap();
        check!(archived);

        mirror(&pool, &template(9, "renamed-away", "Back"))
            .await
            .unwrap();
        check!(!copy_of(&pool, 9).await.unwrap().archived);
    }

    #[tokio::test]
    async fn a_deleted_template_archives_its_copy() {
        let pool = migrated().await;
        for id in 1..=4 {
            mirror(&pool, &template(id, &format!("t{id}"), "Hi"))
                .await
                .unwrap();
        }

        archive_deleted(&pool, &[1, 2, 3]).await.unwrap();

        check!(copy_of(&pool, 4).await.unwrap().archived);
        check!(!copy_of(&pool, 3).await.unwrap().archived);
    }

    #[tokio::test]
    async fn a_list_of_ids_that_would_archive_most_copies_is_refused() {
        let pool = migrated().await;
        for id in 1..=4 {
            mirror(&pool, &template(id, &format!("t{id}"), "Hi"))
                .await
                .unwrap();
        }

        check!(archive_deleted(&pool, &[]).await.is_err());
        check!(archive_deleted(&pool, &[1, 2]).await.is_err());
        check!(!copy_of(&pool, 4).await.unwrap().archived);
    }
}
