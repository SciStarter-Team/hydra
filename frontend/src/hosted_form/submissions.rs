//! A form's submissions, for the people who may see them
//! ([`super::may_see_submissions`]): a page of them, newest first, each as a
//! card of its answers that they can judge, note and delete, as orrery's
//! cards of a project's data let them; and all of them as a CSV.
//!
//! Who made a submission is given by their id; staff alone see their email,
//! and the network address it came from, as orrery showed the people running
//! a form no more.

use std::{collections::HashMap, sync::Arc};

use anyhow::Error;
use common::{
    message::{
        form::{
            Answer, Evaluation, FieldKey, FormDefinition, FormLookup, SubmissionOf,
            SubmissionRecord, SubmissionsPage, SubmissionsRequest, Verdict,
        },
        location::Point,
        user::BasicInfo,
    },
    service::request,
};
use jiff::{Timestamp, tz::TimeZone};
use poem::{
    IntoResponse, Response, handler,
    http::StatusCode,
    web::{Form, Path, Query, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use super::answers::{self, Display};
use crate::{
    component::{Component, Head, StandardLayout},
    middleware::authorization::Authenticated,
};

/// How many submissions a page of them shows.
const PER_PAGE: u32 = 25;

/// How many submissions the CSV asks for at a time.
const CSV_PAGE: u32 = 1000;

/// Someone who may see a form's submissions, and the form.
struct Reader {
    form: FormDefinition,
    user: Arc<BasicInfo>,
}

impl Reader {
    fn is_staff(&self) -> bool {
        self.user.is_staff || self.user.is_superuser
    }
}

/// The form `form_id` and its reader, or what to answer someone who may not
/// read it: a sign-in for someone not signed in, and for anyone else no such
/// page, so that the form's submissions are not even said to exist.
async fn reader(
    form_id: Ulid,
    user: Option<Arc<BasicInfo>>,
    here: &str,
) -> Result<Result<Reader, Response>, Error> {
    let Some(user) = user else {
        let login = format!("/login?next={}", urlencoding::encode(here));
        return Ok(Err(Redirect::see_other(login).into_response()));
    };

    let Some(form) = super::load(FormLookup::Id(form_id)).await? else {
        return Ok(Err(StatusCode::NOT_FOUND.into_response()));
    };

    if !super::may_see_submissions(&form, Some(&user)).await? {
        return Ok(Err(StatusCode::NOT_FOUND.into_response()));
    }

    Ok(Ok(Reader { form, user }))
}

/// The emails of those of `submissions`' submitters who are users, for
/// staff.
async fn emails(submissions: &[SubmissionRecord]) -> Result<HashMap<Ulid, String>, Error> {
    let mut submitters: Vec<Ulid> = submissions
        .iter()
        .filter_map(|submission| submission.submitter)
        .collect();
    submitters.sort();
    submitters.dedup();

    Ok(crate::admin::autocomplete::resolve_user_names(submitters)
        .await?
        .into_iter()
        .collect())
}

/// When a submission was made, to the minute, in UTC.
fn when_made(record: &SubmissionRecord) -> String {
    Timestamp::try_from(record.when.clone())
        .map(|when| {
            when.to_zoned(TimeZone::UTC)
                .strftime("%Y-%m-%d %H:%M UTC")
                .to_string()
        })
        .unwrap_or_else(|_| record.when.to_string())
}

fn place(point: Option<Point>) -> String {
    point.map_or_else(String::new, |point| {
        format!("{}, {}", point.latitude, point.longitude)
    })
}

#[derive(TemplateOnce)]
#[template(path = "hosted_form/submissions.stpl.html")]
struct SubmissionsPageTemplate {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<SubmissionsContent> as Component>::Prepared,
    title: String,
}

#[derive(TemplateOnce)]
#[template(path = "hosted_form/submissions_content.stpl.html")]
struct SubmissionsContent {
    form: FormDefinition,
    /// The form's own page.
    form_path: String,
    cards: Vec<SubmissionCard>,
    total: u32,
    offset: u32,
}

impl SubmissionsContent {
    fn newer(&self) -> Option<u32> {
        (self.offset > 0).then(|| self.offset.saturating_sub(PER_PAGE))
    }

    fn older(&self) -> Option<u32> {
        let next = self.offset + self.cards.len() as u32;
        (next < self.total).then_some(next)
    }
}

/// One submission, as its card shows it.
#[derive(TemplateOnce)]
#[template(path = "hosted_form/submission_card.stpl.html")]
struct SubmissionCard {
    form_id: Ulid,
    submission_id: Ulid,
    when: String,
    submitter: Option<Ulid>,
    /// Staff only.
    email: Option<String>,
    location: String,
    ip_location: String,
    /// Staff only.
    ip_address: Option<String>,
    /// Each question the submission answered, in the form's order, and the
    /// answer.
    answers: Vec<(String, Display)>,
    /// What it answered that no question of the form asks any longer.
    other: Vec<(FieldKey, String)>,
    evaluation: EvaluationForm,
}

impl SubmissionCard {
    fn new(
        form: &FormDefinition,
        record: &SubmissionRecord,
        email: Option<String>,
        staff: bool,
    ) -> Self {
        let answers = form
            .fields
            .iter()
            .filter(|field| field.kind.asks())
            .filter_map(|field| {
                let answer = record.answers.get(&field.key)?;
                Some((field.label.clone(), answers::display(Some(field), answer)))
            })
            .collect();

        let other = form
            .unmatched(&record.answers)
            .map(|(key, answer)| (key.clone(), answers::written(answer)))
            .collect();

        SubmissionCard {
            form_id: form.form_id,
            submission_id: record.submission_id,
            when: when_made(record),
            submitter: record.submitter,
            email,
            location: place(record.location),
            ip_location: place(record.ip_location),
            ip_address: staff.then(|| record.ip_address.clone()).flatten(),
            answers,
            other,
            evaluation: EvaluationForm {
                form_id: form.form_id,
                submission_id: record.submission_id,
                evaluation: record.evaluation,
                comment: record.comment.clone(),
                saved: false,
            },
        }
    }
}

/// A submission's judgement and note, as a form to change them.
#[derive(TemplateOnce)]
#[template(path = "hosted_form/submission_evaluation.stpl.html")]
struct EvaluationForm {
    form_id: Ulid,
    submission_id: Ulid,
    evaluation: Option<Verdict>,
    comment: String,
    /// Whether they were just saved.
    saved: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct TableQuery {
    #[serde(default)]
    offset: u32,
}

/// GET `/-form/:form_id/submissions`: a page of the form's submissions,
/// newest first.
#[handler]
pub(super) async fn table(
    Authenticated(user): Authenticated,
    Path(form_id): Path<Ulid>,
    Query(query): Query<TableQuery>,
) -> Result<Response, Error> {
    let here = format!("/-form/{form_id}/submissions");
    let reader = match reader(form_id, user, &here).await? {
        Ok(reader) => reader,
        Err(response) => return Ok(response),
    };

    let page: SubmissionsPage = request(
        "user.forms.submissions",
        SubmissionsRequest {
            form_id,
            submitter: None,
            after: None,
            offset: query.offset,
            limit: PER_PAGE,
        },
    )
    .await?;

    let staff = reader.is_staff();
    // A card is still worth showing without its submitter's email.
    let mut emails = if staff {
        emails(&page.submissions).await.unwrap_or_else(|e| {
            tracing::warn!("Couldn't look up the emails of submitters to {form_id}: {e:#}");
            HashMap::new()
        })
    } else {
        HashMap::new()
    };

    let cards = page
        .submissions
        .iter()
        .map(|record| {
            let email = record
                .submitter
                .and_then(|submitter| emails.remove(&submitter));
            SubmissionCard::new(&reader.form, record, email, staff)
        })
        .collect();

    let title = format!("Submissions: {}", reader.form.name);
    let content = SubmissionsContent {
        form_path: super::page::page_path(form_id).await?,
        form: reader.form,
        cards,
        total: page.total,
        offset: query.offset,
    };

    let page = SubmissionsPageTemplate {
        head: Head::default().prepare(None).await?,
        layout: StandardLayout::new(here, content)
            .prepare(Some(reader.user))
            .await?,
        title,
    };

    Ok(Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(page.render_once()?))
}

#[derive(Debug, Deserialize)]
pub(super) struct EvaluationInput {
    #[serde(default)]
    evaluation: String,
    #[serde(default)]
    comment: String,
}

/// POST `/-form/:form_id/submissions/:submission_id/evaluation`: judge a
/// submission and note why, answered with the form to do so again.
#[handler]
pub(super) async fn evaluate(
    Authenticated(user): Authenticated,
    Path((form_id, submission_id)): Path<(Ulid, Ulid)>,
    Form(input): Form<EvaluationInput>,
) -> Result<Response, Error> {
    let here = format!("/-form/{form_id}/submissions");
    if let Err(response) = reader(form_id, user, &here).await? {
        return Ok(response);
    }

    let evaluation = match input.evaluation.as_str() {
        "" => None,
        verdict => Some(verdict.parse::<Verdict>().map_err(Error::msg)?),
    };
    let comment = input.comment.trim().to_string();

    let judged: bool = request(
        "user.forms.evaluate",
        Evaluation {
            submission: SubmissionOf {
                form_id,
                submission_id,
            },
            evaluation,
            comment: comment.clone(),
        },
    )
    .await?;

    if !judged {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }

    Ok(Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(
            EvaluationForm {
                form_id,
                submission_id,
                evaluation,
                comment,
                saved: true,
            }
            .render_once()?,
        ))
}

/// DELETE `/-form/:form_id/submissions/:submission_id`: delete a submission
/// for good.
#[handler]
pub(super) async fn delete(
    Authenticated(user): Authenticated,
    Path((form_id, submission_id)): Path<(Ulid, Ulid)>,
) -> Result<Response, Error> {
    let here = format!("/-form/{form_id}/submissions");
    if let Err(response) = reader(form_id, user, &here).await? {
        return Ok(response);
    }

    let deleted: bool = request(
        "user.forms.delete_submission",
        SubmissionOf {
            form_id,
            submission_id,
        },
    )
    .await?;

    Ok(if deleted {
        Response::builder()
            .content_type("text/html; charset=utf-8")
            .body("")
    } else {
        StatusCode::NOT_FOUND.into_response()
    })
}

/// The CSV's columns before the answers', and after.
fn csv_header(form: &FormDefinition, staff: bool) -> Vec<String> {
    let mut header: Vec<String> = ["submission", "orrery_id", "submitted", "submitter"]
        .into_iter()
        .map(str::to_string)
        .collect();

    if staff {
        header.push("submitter_email".to_string());
    }

    header.extend(
        ["latitude", "longitude", "ip_latitude", "ip_longitude"]
            .into_iter()
            .map(str::to_string),
    );

    if staff {
        header.push("ip_address".to_string());
    }

    header.extend(
        form.fields
            .iter()
            .filter(|field| field.kind.asks())
            .map(|field| field.label.clone()),
    );
    header.extend(
        ["other_answers", "evaluation", "comment"]
            .into_iter()
            .map(str::to_string),
    );

    header
}

/// One submission as a row of the CSV, in [`csv_header`]'s order.
fn csv_row(
    form: &FormDefinition,
    record: &SubmissionRecord,
    email: Option<&str>,
    staff: bool,
) -> Vec<String> {
    let coordinates = |point: Option<Point>| match point {
        Some(point) => [point.latitude.to_string(), point.longitude.to_string()],
        None => [String::new(), String::new()],
    };

    let mut row = vec![
        record.submission_id.to_string(),
        record
            .orrery_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        record.when.to_string(),
        record
            .submitter
            .map(|id| id.to_string())
            .unwrap_or_default(),
    ];

    if staff {
        row.push(email.unwrap_or_default().to_string());
    }

    row.extend(coordinates(record.location));
    row.extend(coordinates(record.ip_location));

    if staff {
        row.push(record.ip_address.clone().unwrap_or_default());
    }

    row.extend(
        form.fields
            .iter()
            .filter(|field| field.kind.asks())
            .map(|field| {
                record
                    .answers
                    .get(&field.key)
                    .map(answers::written)
                    .unwrap_or_default()
            }),
    );

    let other: Vec<String> = form
        .unmatched(&record.answers)
        .map(|(key, answer): (&FieldKey, &Answer)| format!("{key}: {}", answers::written(answer)))
        .collect();
    row.push(other.join("; "));
    row.push(
        record
            .evaluation
            .map(|verdict| verdict.as_str().to_string())
            .unwrap_or_default(),
    );
    row.push(record.comment.clone());

    row
}

fn csv_line(fields: &[String]) -> Result<String, Error> {
    let mut writer = ::csv::WriterBuilder::new()
        .terminator(::csv::Terminator::CRLF)
        .from_writer(Vec::new());
    writer.write_record(fields)?;

    Ok(String::from_utf8(writer.into_inner()?)?)
}

/// How far the CSV of a form's submissions has been read.
struct Cursor {
    /// The last submission read.
    after: Option<Ulid>,
    done: bool,
}

/// What the CSV of `form_id`'s submissions is when its next part could not
/// be made: an error, which ends the download as failed rather than as a CSV
/// that looks whole and is not.
fn failed(form_id: Ulid, e: Error) -> (Result<String, std::io::Error>, Cursor) {
    tracing::error!("Couldn't write the submissions of {form_id}: {e:#}");

    (
        Err(std::io::Error::other(e.to_string())),
        Cursor {
            after: None,
            done: true,
        },
    )
}

/// GET `/-form/:form_id/submissions.csv`: every submission of the form, newest
/// first, a column for each question in the form's order. Read a page at a
/// time, each after the last of the one before, so that submissions made as
/// it is read neither repeat nor push any out.
#[handler]
pub(super) async fn csv(
    Authenticated(user): Authenticated,
    Path(form_id): Path<Ulid>,
) -> Result<Response, Error> {
    let here = format!("/-form/{form_id}/submissions.csv");
    let reader = match reader(form_id, user, &here).await? {
        Ok(reader) => reader,
        Err(response) => return Ok(response),
    };

    let staff = reader.is_staff();
    let form = Arc::new(reader.form);
    let header = csv_line(&csv_header(&form, staff))?;
    let filename = format!("form-{form_id}-submissions.csv");

    let pages = futures::stream::unfold(
        Cursor {
            after: None,
            done: false,
        },
        move |cursor| {
            let form = form.clone();

            async move {
                if cursor.done {
                    return None;
                }

                let page: Result<SubmissionsPage, Error> = request(
                    "user.forms.submissions",
                    SubmissionsRequest {
                        form_id,
                        submitter: None,
                        after: cursor.after,
                        offset: 0,
                        limit: CSV_PAGE,
                    },
                )
                .await;

                let page = match page {
                    Ok(page) => page,
                    Err(e) => return Some(failed(form_id, e)),
                };

                let Some(last) = page.submissions.last() else {
                    return None;
                };
                let after = Some(last.submission_id);

                let emails = if staff {
                    match emails(&page.submissions).await {
                        Ok(emails) => emails,
                        Err(e) => return Some(failed(form_id, e)),
                    }
                } else {
                    HashMap::new()
                };

                let mut chunk = String::new();
                for record in &page.submissions {
                    let email = record
                        .submitter
                        .and_then(|submitter| emails.get(&submitter))
                        .map(String::as_str);

                    match csv_line(&csv_row(&form, record, email, staff)) {
                        Ok(line) => chunk.push_str(&line),
                        Err(e) => return Some(failed(form_id, e)),
                    }
                }

                Some((Ok(chunk), Cursor { after, done: false }))
            }
        },
    );

    let body = futures::StreamExt::chain(
        futures::stream::once(async move { Ok::<_, std::io::Error>(header) }),
        pages,
    );

    Ok(Response::builder()
        .content_type("text/csv; charset=utf-8")
        .header(
            "Content-Disposition",
            format!(r#"attachment; filename="{filename}""#),
        )
        .body(poem::Body::from_bytes_stream(body)))
}

#[cfg(test)]
mod tests;
