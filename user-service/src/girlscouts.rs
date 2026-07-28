//! Girl Scouts Take Action submissions.
//!
//! Ported from orrery's `girlscouts_action_save`. Troops log a Take Action
//! project at hydra's `/girlscouts-action` --- the last step of the Think Like
//! a Citizen Scientist Journey --- and staff read the write-ups at
//! `/-admin/girlscouts`. Nothing here is published; these are records for
//! staff, kept next to the user records they belong to rather than in the
//! frontend, which has no reason to hold or peer them.

use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::dashboard::{
        TakeActionList, TakeActionListQuery, TakeActionRow, TakeActionSubmission,
    },
    service::{decode_structured_message, log_activity},
    shared::db,
    timestring::TimeString,
    ulid::AsUlid,
};
use ulid::Ulid;

/// Record a Take Action write-up. Returns the new submission's id.
pub async fn take_action_submit(req: &Request) -> Result<Ulid, Error> {
    let message: TakeActionSubmission = decode_structured_message(req.message.clone())?;

    let submission_id = Ulid::new();
    let submission_id_str = submission_id.to_string();
    let user_id_str = message.user_id.to_string();
    let submitted = TimeString::now().to_string();

    sqlx::query!(
        r#"INSERT INTO girlscouts_take_action
           (submission_id, submitted, user_id, troop, problem,
            solution, sustainable, photo)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
        submission_id_str,
        submitted,
        user_id_str,
        message.troop,
        message.problem,
        message.solution,
        message.sustainable,
        message.photo,
    )
    .execute(db())
    .await?;

    log_activity(
        None,
        "girlscouts_take_action",
        Some(message.user_id),
        Some(submission_id),
        None::<String>,
    )
    .await?;

    Ok(submission_id)
}

/// One page of the staff queue, newest first. The submitter is joined here so
/// the admin needs one call rather than one profile lookup per row.
pub async fn take_action_list(req: &Request) -> Result<TakeActionList, Error> {
    let query: TakeActionListQuery = decode_structured_message(req.message.clone())?;

    let total: i64 = sqlx::query_scalar!(r#"SELECT COUNT(*) FROM girlscouts_take_action"#)
        .fetch_one(db())
        .await? as i64;

    let rows = sqlx::query!(
        r#"SELECT
            g.submission_id AS "submission_id!",
            g.submitted AS "submitted!",
            g.user_id AS "user_id!",
            u.first_name AS "first_name!",
            u.last_name AS "last_name!",
            u.handle AS "handle!",
            u.primary_email AS "primary_email!",
            g.troop AS "troop!",
            g.problem AS "problem!",
            g.solution AS "solution!",
            g.sustainable AS "sustainable!",
            g.photo AS "photo!"
        FROM "girlscouts_take_action" g
        JOIN "users" u ON u.user_id = g.user_id
        ORDER BY g.submission_id DESC
        LIMIT ? OFFSET ?"#,
        query.limit,
        query.offset,
    )
    .fetch_all(db())
    .await?;

    let rows = rows
        .into_iter()
        .map(|row| TakeActionRow {
            submission_id: row.submission_id.as_ulid().unwrap_or_default(),
            user_id: row.user_id.as_ulid().unwrap_or_default(),
            submitted: row.submitted,
            name: format!("{} {}", row.first_name, row.last_name)
                .trim()
                .to_string(),
            handle: row.handle,
            email: row.primary_email,
            troop: row.troop,
            problem: row.problem,
            solution: row.solution,
            sustainable: row.sustainable,
            photo: row.photo,
        })
        .collect();

    Ok(TakeActionList { rows, total })
}

/// Discard a submission. Staff-only; the frontend gates the route.
pub async fn take_action_delete(req: &Request) -> Result<(), Error> {
    let submission_id: Ulid = decode_structured_message(req.message.clone())?;
    let submission_id_str = submission_id.to_string();

    sqlx::query!(
        "DELETE FROM girlscouts_take_action WHERE submission_id = ?",
        submission_id_str
    )
    .execute(db())
    .await?;

    Ok(())
}
