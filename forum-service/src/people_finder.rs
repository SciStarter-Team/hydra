//! People Finder requests (`forum.people_finder.*`): a message to everyone in
//! an area, held until staff decide it, then delivered as one batch of
//! ordinary direct messages. The whole tool is described in
//! `common::message::people_finder`; this is orrery's `PeopleFinderApproval`.
//!
//! Delivery is the slow part -- one dense area holds tens of thousands of
//! people -- so it is never done inside a request. Approving a request wakes
//! [`delivery_worker`], the one task that delivers, which works through every
//! approved request not yet delivered. Being the only one is what keeps a copy
//! from being made twice, and it finds whatever a restart cut short.

use std::collections::BTreeSet;
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Error, anyhow};
use async_nats::service::Request;
use common::{
    message::people_finder::{
        PeopleFinderArea, PeopleFinderAudience, PeopleFinderDecided, PeopleFinderDecision,
        PeopleFinderFilter, PeopleFinderPendingRequest, PeopleFinderReach,
        PeopleFinderRecipientsPage, PeopleFinderRecipientsRequest, PeopleFinderSubmission,
        PeopleFinderSubmitted, PeopleFinderVerdict,
    },
    service::{TriggerEvent, request_with_retry, trigger_email},
    shared::db,
    timestring::TimeString,
};
use tokio::sync::Notify;
use ulid::Ulid;

use crate::messaging::{
    NameAudience, check_sender_may_send, create_conversation, escape_html, resolve_participant_name,
};

/// Where staff review requests.
const REVIEW_URL: &str = "https://scistarter.org/-admin/messages";

/// How many recipients to ask user-service for at a time.
const RECIPIENT_PAGE: i64 = 1000;

/// How long the worker sleeps when nobody wakes it, before looking again for
/// a delivery that failed part way.
const RETRY_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// Wakes [`delivery_worker`] when a request is approved.
static APPROVED: LazyLock<Notify> = LazyLock::new(Notify::new);

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::AsRefStr, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
enum Status {
    Pending,
    Approved,
    Denied,
}

impl From<PeopleFinderVerdict> for Status {
    fn from(verdict: PeopleFinderVerdict) -> Status {
        match verdict {
            PeopleFinderVerdict::Approve => Status::Approved,
            PeopleFinderVerdict::Deny => Status::Denied,
        }
    }
}

fn audience_from_row(
    latitude: f64,
    longitude: f64,
    radius_miles: f64,
    filters: &str,
) -> Result<PeopleFinderAudience, Error> {
    let area = PeopleFinderArea::new(latitude, longitude, radius_miles).ok_or_else(|| {
        anyhow!("Stored People Finder area is not a place: {latitude}, {longitude}")
    })?;
    let filters: BTreeSet<PeopleFinderFilter> = serde_json::from_str(filters)?;

    Ok(PeopleFinderAudience { area, filters })
}

/// Plain text as it should read in an HTML email: escaped, with its line
/// breaks kept.
fn text_html(text: &str) -> String {
    escape_html(text).replace('\n', "<br>")
}

/// `forum.people_finder.submit`: hold a request for review, and tell the
/// reviewers. Whoever binds the `people-finder-new-request` trigger is told;
/// the binding seeded by emailer-service names orrery's approvers.
pub async fn submit(req: &Request) -> Result<PeopleFinderSubmitted, Error> {
    let submission: PeopleFinderSubmission = flexbuffers::from_slice(&req.message.payload)?;

    check_sender_may_send(submission.sender_id).await?;

    let request_id = Ulid::new();
    let request_id_str = request_id.to_string();
    let sender_id = submission.sender_id.to_string();
    let now = TimeString::now().to_string();
    let area = submission.audience.area;
    let filters = serde_json::to_string(&submission.audience.filters)?;
    let reach = submission.reach.as_ref();
    let status = Status::Pending.as_ref();

    sqlx::query!(
        r#"INSERT INTO "people_finder_requests"
             ("request_id", "sender_id", "created_at", "latitude", "longitude", "radius_miles",
              "filters", "reach", "subject", "body", "status")
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        request_id_str,
        sender_id,
        now,
        area.latitude,
        area.longitude,
        area.radius_miles,
        filters,
        reach,
        submission.subject,
        submission.body,
        status,
    )
    .execute(db())
    .await?;

    let sender = resolve_participant_name(&sender_id, NameAudience::Staff)
        .await
        .unwrap_or_else(|| sender_id.clone());

    let fields = [
        ("sender".to_string(), escape_html(&sender)),
        (
            "audience".to_string(),
            escape_html(&submission.audience.describe(submission.reach)),
        ),
        (
            "message_subject".to_string(),
            escape_html(&submission.subject),
        ),
        ("message_body".to_string(), text_html(&submission.body)),
        ("review_url".to_string(), REVIEW_URL.to_string()),
    ];

    if let Err(err) = trigger_email(
        TriggerEvent::PeopleFinderNewRequest,
        submission.sender_id,
        Some(&fields),
    )
    .await
    {
        tracing::error!(%request_id, "Failed to notify People Finder reviewers: {err}");
    }

    Ok(PeopleFinderSubmitted { request_id })
}

/// `forum.people_finder.pending`: every request awaiting review, oldest first.
pub async fn pending(_req: &Request) -> Result<Vec<PeopleFinderPendingRequest>, Error> {
    let status = Status::Pending.as_ref();
    let rows = sqlx::query!(
        r#"SELECT "request_id" AS "request_id!", "sender_id", "created_at", "latitude",
                  "longitude", "radius_miles", "filters", "reach", "subject", "body"
             FROM "people_finder_requests"
            WHERE "status" = ?
            ORDER BY "created_at""#,
        status,
    )
    .fetch_all(db())
    .await?;

    let mut out = Vec::with_capacity(rows.len());

    for row in rows {
        let audience =
            audience_from_row(row.latitude, row.longitude, row.radius_miles, &row.filters)?;
        let sender = resolve_participant_name(&row.sender_id, NameAudience::Staff)
            .await
            .unwrap_or_else(|| row.sender_id.clone());
        let when = TimeString::try_from(row.created_at.clone())
            .map(|t| t.format_display())
            .unwrap_or(row.created_at);

        out.push(PeopleFinderPendingRequest {
            request_id: row.request_id.parse()?,
            sender_id: row.sender_id.parse()?,
            sender,
            when,
            audience,
            reach: row.reach.parse()?,
            subject: row.subject,
            body: row.body,
        });
    }

    Ok(out)
}

/// `forum.people_finder.review`: settle a pending request. Only the first
/// decision counts, so two reviewers acting at once cannot both approve it.
pub async fn review(req: &Request) -> Result<PeopleFinderDecided, Error> {
    let decision: PeopleFinderDecision = flexbuffers::from_slice(&req.message.payload)?;

    let status = Status::from(decision.verdict);
    let status_str = status.as_ref();
    let pending = Status::Pending.as_ref();
    let request_id = decision.request_id.to_string();
    let reviewer_id = decision.reviewer_id.to_string();
    let now = TimeString::now().to_string();
    let batch = (status == Status::Approved).then(|| Ulid::new().to_string());

    let settled = sqlx::query!(
        r#"UPDATE "people_finder_requests"
              SET "status" = ?, "decided_at" = ?, "decided_by" = ?, "batch" = ?
            WHERE "request_id" = ? AND "status" = ?"#,
        status_str,
        now,
        reviewer_id,
        batch,
        request_id,
        pending,
    )
    .execute(db())
    .await?
    .rows_affected()
        == 1;

    if settled && status == Status::Approved {
        APPROVED.notify_one();
    }

    Ok(PeopleFinderDecided { decided: settled })
}

/// The one task that delivers approved requests. Runs for the life of the
/// service.
pub async fn delivery_worker() {
    loop {
        if let Err(err) = deliver_approved().await {
            tracing::error!("People Finder delivery failed, will retry: {err:#}");
        }

        let _ = tokio::time::timeout(RETRY_INTERVAL, APPROVED.notified()).await;
    }
}

/// Deliver every approved request that has not been delivered yet, oldest
/// decision first.
async fn deliver_approved() -> Result<(), Error> {
    let approved = Status::Approved.as_ref();
    let undelivered = sqlx::query_scalar!(
        r#"SELECT "request_id" AS "request_id!"
             FROM "people_finder_requests"
            WHERE "status" = ? AND "delivered_at" IS NULL
            ORDER BY "decided_at""#,
        approved,
    )
    .fetch_all(db())
    .await?;

    for request_id in undelivered {
        deliver(&request_id).await?;
    }

    Ok(())
}

/// Make a copy of one approved request for each of its recipients, then
/// stamp it delivered and tell the sender, as orrery's
/// `PeopleFinderApproval.send` does.
///
/// Recipients come in `user_id` order, and a copy's topic lives in the
/// recipient's inbox, so the last inbox holding a copy from this batch is how
/// far an earlier attempt got. Starting after it lets a delivery cut short
/// carry on without sending anybody a second copy.
async fn deliver(request_id: &str) -> Result<(), Error> {
    let row = sqlx::query!(
        r#"SELECT "sender_id", "latitude", "longitude", "radius_miles", "filters", "reach",
                  "subject", "body", "batch" AS "batch!"
             FROM "people_finder_requests"
            WHERE "request_id" = ?"#,
        request_id,
    )
    .fetch_one(db())
    .await?;

    let audience = audience_from_row(row.latitude, row.longitude, row.radius_miles, &row.filters)?;
    let reach: PeopleFinderReach = row.reach.parse()?;
    let sender_id: Ulid = row.sender_id.parse()?;

    let reached = sqlx::query_scalar!(
        r#"SELECT MAX(t."forum_id") AS "forum_id: String"
             FROM "posts" p
             JOIN "topics" t ON t."topic_id" = p."topic_id"
            WHERE p."batch" = ?"#,
        row.batch,
    )
    .fetch_one(db())
    .await?;

    let mut after: Option<Ulid> = reached.and_then(|id| id.parse().ok());

    let operators: Vec<Ulid> = request_with_retry("frontend.people_finder.operators", ()).await?;

    loop {
        let page: PeopleFinderRecipientsPage = request_with_retry(
            "user.people_finder.recipients",
            PeopleFinderRecipientsRequest {
                audience: audience.clone(),
                reach,
                operators: operators.clone(),
                after,
                limit: RECIPIENT_PAGE,
            },
        )
        .await?;

        let last = (page.user_ids.len() as i64) < RECIPIENT_PAGE;

        for to_id in page.user_ids {
            after = Some(to_id);

            // The sender may well live in the area they are writing to.
            if to_id == sender_id {
                continue;
            }

            // One recipient who cannot be written to must not hold up the
            // rest, as in `send_bulk`.
            if let Err(err) = create_conversation(
                to_id,
                sender_id,
                &row.subject,
                &row.body,
                None,
                1,
                Some(&row.batch),
            )
            .await
            {
                tracing::error!(%request_id, %to_id, "People Finder copy not delivered: {err}");
            }
        }

        if last {
            break;
        }
    }

    let now = TimeString::now().to_string();
    sqlx::query!(
        r#"UPDATE "people_finder_requests" SET "delivered_at" = ? WHERE "request_id" = ?"#,
        now,
        request_id,
    )
    .execute(db())
    .await?;

    if let Err(err) = trigger_email(TriggerEvent::PeopleFinderConfirmation, sender_id, None).await {
        tracing::error!(%request_id, "Failed to confirm People Finder delivery: {err}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_reads_back_as_itself() {
        for status in [Status::Pending, Status::Approved, Status::Denied] {
            assert_eq!(status.as_ref().parse::<Status>().ok(), Some(status));
        }
    }

    #[test]
    fn a_stored_audience_reads_back() {
        let audience = PeopleFinderAudience {
            area: PeopleFinderArea::new(39.0, -98.0, 50.0).unwrap(),
            filters: [PeopleFinderFilter::Champions].into_iter().collect(),
        };
        let filters = serde_json::to_string(&audience.filters).unwrap();

        assert_eq!(filters, r#"["champions"]"#);
        assert_eq!(
            audience_from_row(39.0, -98.0, 50.0, &filters).unwrap(),
            audience
        );
    }

    #[test]
    fn message_text_is_escaped_and_keeps_its_lines() {
        assert_eq!(text_html("a < b\nc & d"), "a &lt; b<br>c &amp; d");
    }
}
