//! What people submit through forms: taking a submission, and reading,
//! reviewing and mapping those kept.
//!
//! A submission is held to its form by [`FormDefinition::read_submission`]
//! before it is kept. The place a submitter picked and the place their
//! address was looked up to be are kept apart, so that neither passes for
//! the other.

use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::{
        dashboard::EntitySummary,
        email::{SendEmailResponse, SendStoredEmailRequest},
        form::{
            Answers, Evaluation, FormDefinition, FormLookup, FormSubmission, MapMarker,
            RememberedRequest, SubmissionOf, SubmissionRecord, SubmissionsPage, SubmissionsRequest,
            SubmitOutcome, Verdict, latest_date,
        },
        location::{LocationInfo, Point},
        log::ProjectParticipationRecord,
        user::JoinSource,
    },
    service::{decode_structured_message, request, send},
    shared::db,
    timestring::TimeString,
    transaction::Immediate,
    ulid::AsUlid,
};
use sqlx::{Executor, Sqlite, SqlitePool};
use ulid::Ulid;

/// The most contributions one submission credits, however many its
/// repetitions field claims: each is a record of its own.
const MOST_REPETITIONS: u32 = 1000;

/// The most submissions a page of them holds.
const MOST_PER_PAGE: u32 = 1000;

/// The most bytes of answers a page of submissions holds, past its first:
/// well inside NATS's one-megabyte message, with room for the rest of each
/// record and for encoding.
const PAGE_ANSWER_BYTES: usize = 400_000;

/// The most markers a form's map shows: its latest submissions.
const MOST_MARKERS: i64 = 2500;

/// The most bytes a form's markers take, reckoned as each one's answers
/// and [`MARKER_BYTES`] besides: well inside NATS's one-megabyte message.
const MAP_BYTES: usize = 600_000;

/// What a marker takes besides its answers, at most: its place and time.
const MARKER_BYTES: usize = 96;

/// Keep `record` as a submission of the form `form_id`, inside the caller's
/// transaction. A copy of orrery's replaces the one kept under its orrery id.
pub(super) async fn store<'c>(
    conn: impl Executor<'c, Database = Sqlite>,
    form_id: Ulid,
    record: &SubmissionRecord,
) -> Result<(), Error> {
    let submission_id = record.submission_id.to_string();
    let form_id = form_id.to_string();
    let submitter = record.submitter.map(|submitter| submitter.to_string());
    let answers = serde_json::to_string(&record.answers)?;
    let location = record.location.map(|point| point.to_string());
    let ip_location = record.ip_location.map(|point| point.to_string());

    sqlx::query!(
        r#"INSERT INTO "form_submissions"
             ("submission_id", "form_id", "orrery_id", "submitter", "when", "answers",
              "location", "ip_address", "ip_location", "evaluation", "comment")
           VALUES (?, ?, ?, ?, ?, ?, SetSRID(GeomFromGeoJSON(?), 4326), ?,
                   SetSRID(GeomFromGeoJSON(?), 4326), ?, ?)
           ON CONFLICT ("orrery_id") DO UPDATE SET
             "form_id" = excluded."form_id",
             "submitter" = excluded."submitter",
             "when" = excluded."when",
             "answers" = excluded."answers",
             "location" = excluded."location",
             "ip_address" = excluded."ip_address",
             "ip_location" = excluded."ip_location",
             "evaluation" = excluded."evaluation",
             "comment" = excluded."comment""#,
        submission_id,
        form_id,
        record.orrery_id,
        submitter,
        record.when,
        answers,
        location,
        record.ip_address,
        ip_location,
        record.evaluation,
        record.comment,
    )
    .execute(conn)
    .await?;

    Ok(())
}

/// A point read back from its two coordinates, where it has both.
fn point(longitude: Option<f64>, latitude: Option<f64>) -> Option<Point> {
    Some(Point {
        latitude: latitude?,
        longitude: longitude?,
    })
}

/// Whether `user_id` has submitted the form `form_id` before.
async fn submitted_before<'c>(
    conn: impl Executor<'c, Database = Sqlite>,
    form_id: &str,
    user_id: &str,
) -> Result<bool, Error> {
    Ok(sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM "form_submissions"
                          WHERE "form_id" = ? AND "submitter" = ?)
             AS "before!: bool""#,
        form_id,
        user_id
    )
    .fetch_one(conn)
    .await?)
}

/// `user.forms.submit`: hold a submission to its form, and keep it if it
/// passes.
pub async fn submit(req: &Request) -> Result<SubmitOutcome, Error> {
    let submission: FormSubmission = decode_structured_message(req.message.clone())?;

    submit_form(db(), submission).await
}

pub(crate) async fn submit_form(
    pool: &SqlitePool,
    submission: FormSubmission,
) -> Result<SubmitOutcome, Error> {
    let Some(form) = super::load(pool, &FormLookup::Id(submission.form_id)).await? else {
        return Ok(SubmitOutcome::NoSuchForm);
    };

    if form.closed {
        return Ok(SubmitOutcome::Closed);
    }

    if form.sign_in_required && submission.submitter.is_none() {
        return Ok(SubmitOutcome::SignInRequired);
    }

    let answers =
        match form.read_submission(&submission.answers, latest_date(jiff::Timestamp::now())) {
            Ok(answers) => answers,
            Err(errors) => return Ok(SubmitOutcome::Refused(errors)),
        };

    let ip_location = match &submission.ip_address {
        Some(address) => looked_up(address).await,
        None => None,
    };

    let record = SubmissionRecord {
        submission_id: Ulid::new(),
        orrery_id: None,
        submitter: submission.submitter,
        when: TimeString::now(),
        location: form.location(&answers),
        answers,
        ip_address: submission.ip_address,
        ip_location,
        evaluation: None,
        comment: String::new(),
    };

    let form_id = form.form_id.to_string();
    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;
    let first = match record.submitter {
        Some(submitter) => !submitted_before(&mut tx, &form_id, &submitter.to_string()).await?,
        None => false,
    };
    store(&mut tx, form.form_id, &record).await?;
    tx.commit().await?;

    if let Some(submitter) = record.submitter {
        credit(&form, submitter, &record.answers, first).await;
    }

    Ok(SubmitOutcome::Saved {
        submission_id: record.submission_id,
    })
}

/// Where the address `address` is looked up to be, if it is found anywhere:
/// the lookup's answer for an address it does not place is no place at all.
async fn looked_up(address: &str) -> Option<Point> {
    let location: LocationInfo = request("location.ip", address.to_string()).await.ok()?;

    location.place()
}

/// What a submission signed in does beyond being kept, as orrery's form did:
/// it joins its submitter to the form's project, credits them with taking
/// part once for each repetition where the form credits that, and sends the
/// form's first-submission email the first time they submit it. Each is
/// reported where it fails, and none undoes the submission.
async fn credit(form: &FormDefinition, submitter: Ulid, answers: &Answers, first: bool) {
    if let Some(project) = &form.project {
        if let Err(e) = crate::project_joins::record_join(
            submitter,
            &project.project_id,
            JoinSource::Participation,
        )
        .await
        {
            eprintln!(
                "Failed to join {submitter} to {} for a submission of form {}: {e}",
                project.project_id, form.form_id
            );
        }

        if form.credits_participation {
            for _ in 0..form.repetitions(answers).min(MOST_REPETITIONS) {
                let record = ProjectParticipationRecord {
                    user_id: submitter,
                    project_id: project.project_id.clone(),
                    // Origin 4: a form SciStarter hosts.
                    origin: Some(4),
                    when: None,
                    duration: None,
                    where_point: None,
                    magnitude: Some(1),
                    authenticated: Some(true),
                    extra: Some(String::new()),
                    fidelity: None,
                    task_name: Some("collection".to_string()),
                };

                if let Err(e) = send("log.project_participation_record", record).await {
                    eprintln!(
                        "Failed to credit {submitter} for a submission of form {}: {e}",
                        form.form_id
                    );
                    break;
                }
            }
        }
    }

    if first && let Some(slug) = &form.first_submission_email {
        let mut additional_data = Vec::new();

        if let Some(project) = &form.project {
            let summary: Option<EntitySummary> =
                request("frontend.entity.summary_by_id", project.project_id.clone())
                    .await
                    .unwrap_or(None);

            if let Some(summary) = summary {
                additional_data.push((
                    "project_page_url".to_string(),
                    format!("https://{}{}", env!("DOMAIN"), summary.page_path()),
                ));
                additional_data.push(("project_name".to_string(), summary.name));
            }
        }

        let sent: Result<SendEmailResponse, Error> = request(
            "emailer.stored.send",
            SendStoredEmailRequest {
                slug: slug.clone(),
                user_id: submitter,
                additional_data: Some(additional_data),
            },
        )
        .await;

        if let Err(e) = sent {
            eprintln!(
                "Failed to send {slug:?} to {submitter} for their first submission of form {}: {e}",
                form.form_id
            );
        }
    }
}

/// `user.forms.remembered`: the answers someone gave the last time they
/// submitted a form, to the fields it remembers.
pub async fn remembered(req: &Request) -> Result<Answers, Error> {
    let asked: RememberedRequest = decode_structured_message(req.message.clone())?;

    remembered_answers(db(), &asked).await
}

pub(crate) async fn remembered_answers(
    pool: &SqlitePool,
    asked: &RememberedRequest,
) -> Result<Answers, Error> {
    let Some(form) = super::load(pool, &FormLookup::Id(asked.form_id)).await? else {
        return Ok(Answers::new());
    };

    let form_id = asked.form_id.to_string();
    let user_id = asked.user_id.to_string();
    let last = sqlx::query_scalar!(
        r#"SELECT "answers" FROM "form_submissions"
           WHERE "form_id" = ? AND "submitter" = ?
           ORDER BY "when" DESC, "submission_id" DESC
           LIMIT 1"#,
        form_id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    let Some(last) = last else {
        return Ok(Answers::new());
    };

    let mut answers: Answers = serde_json::from_str(&last)?;
    answers.retain(|key, _| {
        form.field(key.as_str())
            .is_some_and(|field| field.remember && field.kind.asks())
    });

    Ok(answers)
}

/// `user.forms.submissions`: a page of a form's submissions, newest first.
pub async fn list(req: &Request) -> Result<SubmissionsPage, Error> {
    let asked: SubmissionsRequest = decode_structured_message(req.message.clone())?;

    page(db(), &asked).await
}

pub(crate) async fn page(
    pool: &SqlitePool,
    asked: &SubmissionsRequest,
) -> Result<SubmissionsPage, Error> {
    let form_id = asked.form_id.to_string();
    let submitter = asked.submitter.map(|submitter| submitter.to_string());
    let after = asked.after.map(|after| after.to_string());
    let limit = asked.limit.min(MOST_PER_PAGE);
    let offset = asked.offset;

    let rows = sqlx::query!(
        r#"SELECT "submission_id" AS "submission_id!", "orrery_id", "submitter",
                  "when" AS "when: TimeString", "answers",
                  ST_X("location") AS "location_x?: f64", ST_Y("location") AS "location_y?: f64",
                  "ip_address",
                  ST_X("ip_location") AS "ip_x?: f64", ST_Y("ip_location") AS "ip_y?: f64",
                  "evaluation" AS "evaluation: Verdict", "comment"
           FROM "form_submissions"
           WHERE "form_id" = ?1 AND (?2 IS NULL OR "submitter" = ?2)
             AND (?5 IS NULL OR ("when", "submission_id") <
                   (SELECT "when", "submission_id" FROM "form_submissions"
                    WHERE "submission_id" = ?5))
           ORDER BY "when" DESC, "submission_id" DESC
           LIMIT ?3 OFFSET ?4"#,
        form_id,
        submitter,
        limit,
        offset,
        after
    )
    .fetch_all(pool)
    .await?;

    let total = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "total!: u32" FROM "form_submissions"
           WHERE "form_id" = ?1 AND (?2 IS NULL OR "submitter" = ?2)"#,
        form_id,
        submitter
    )
    .fetch_one(pool)
    .await?;

    let mut answer_bytes = 0;
    let submissions = rows
        .into_iter()
        .take_while(|row| {
            let first = answer_bytes == 0;
            answer_bytes += row.answers.len().max(1);
            first || answer_bytes <= PAGE_ANSWER_BYTES
        })
        .map(|row| {
            Ok(SubmissionRecord {
                submission_id: row.submission_id.as_ulid()?,
                orrery_id: row.orrery_id,
                submitter: row
                    .submitter
                    .map(|submitter| submitter.as_ulid())
                    .transpose()?,
                when: row.when,
                answers: serde_json::from_str(&row.answers)?,
                location: point(row.location_x, row.location_y),
                ip_address: row.ip_address,
                ip_location: point(row.ip_x, row.ip_y),
                evaluation: row.evaluation,
                comment: row.comment,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;

    Ok(SubmissionsPage { submissions, total })
}

/// `user.forms.evaluate`: set a reviewer's note on a submission. Whether
/// there was the submission.
pub async fn evaluate(req: &Request) -> Result<bool, Error> {
    let evaluation: Evaluation = decode_structured_message(req.message.clone())?;

    judge(db(), &evaluation).await
}

pub(crate) async fn judge(pool: &SqlitePool, evaluation: &Evaluation) -> Result<bool, Error> {
    let submission_id = evaluation.submission.submission_id.to_string();
    let form_id = evaluation.submission.form_id.to_string();

    let updated = sqlx::query!(
        r#"UPDATE "form_submissions" SET "evaluation" = ?, "comment" = ?
           WHERE "submission_id" = ? AND "form_id" = ?"#,
        evaluation.evaluation,
        evaluation.comment,
        submission_id,
        form_id
    )
    .execute(pool)
    .await?
    .rows_affected();

    Ok(updated > 0)
}

/// `user.forms.delete_submission`: delete the submission sent, if it is
/// one of the form sent. Whether there was one.
pub async fn delete(req: &Request) -> Result<bool, Error> {
    let submission: SubmissionOf = decode_structured_message(req.message.clone())?;

    remove(db(), submission).await
}

pub(crate) async fn remove(pool: &SqlitePool, submission: SubmissionOf) -> Result<bool, Error> {
    let submission_id = submission.submission_id.to_string();
    let form_id = submission.form_id.to_string();

    let deleted = sqlx::query!(
        r#"DELETE FROM "form_submissions" WHERE "submission_id" = ? AND "form_id" = ?"#,
        submission_id,
        form_id
    )
    .execute(pool)
    .await?
    .rows_affected();

    Ok(deleted > 0)
}

/// `user.forms.map`: the markers of the public map of the form whose id is
/// sent, its latest submissions that picked a place; none for a form without
/// a map. Each shows only the answers the map shows, never who made it; a
/// form whose submissions are anonymized shows each place only to within a
/// kilometre or so.
pub async fn map(req: &Request) -> Result<Vec<MapMarker>, Error> {
    let form_id: Ulid = decode_structured_message(req.message.clone())?;

    markers(db(), form_id).await
}

pub(crate) async fn markers(pool: &SqlitePool, form_id: Ulid) -> Result<Vec<MapMarker>, Error> {
    let Some(form) = super::load(pool, &FormLookup::Id(form_id)).await? else {
        return Ok(Vec::new());
    };

    let Some(map) = &form.map else {
        return Ok(Vec::new());
    };

    let form_id = form_id.to_string();
    let rows = sqlx::query!(
        r#"SELECT "when" AS "when: TimeString", "answers",
                  ST_X("location") AS "longitude!: f64", ST_Y("location") AS "latitude!: f64"
           FROM "form_submissions"
           WHERE "form_id" = ? AND "location" IS NOT NULL
           ORDER BY "when" DESC, "submission_id" DESC
           LIMIT ?"#,
        form_id,
        MOST_MARKERS
    )
    .fetch_all(pool)
    .await?;

    let blur = |coordinate: f64| {
        if form.anonymized {
            (coordinate * 100.0).round() / 100.0
        } else {
            coordinate
        }
    };

    let mut bytes = 0;
    rows.into_iter()
        .map(|row| {
            let mut answers: Answers = serde_json::from_str(&row.answers)?;
            answers.retain(|key, _| map.popup_fields.contains(key));
            bytes += serde_json::to_string(&answers)?.len() + MARKER_BYTES;

            Ok((
                bytes,
                MapMarker {
                    location: Point {
                        latitude: blur(row.latitude),
                        longitude: blur(row.longitude),
                    },
                    when: row.when,
                    answers,
                },
            ))
        })
        // An error goes on to be collected, and reported.
        .take_while(|marker| match marker {
            Ok((bytes, _)) => *bytes <= MAP_BYTES,
            Err(_) => true,
        })
        .map(|marker| marker.map(|(_, marker)| marker))
        .collect()
}
