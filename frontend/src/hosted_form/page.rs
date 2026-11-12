//! A form's page: the form, to fill in and send; the thanks once it is sent;
//! and, where the form has one, the map of where submissions were made.
//!
//! The page works as a plain HTML form: it posts to `/-form/<form id>`,
//! which answers a refused submission with the page again, every answer as
//! it was sent and each refusal beside its question, and a saved one with a
//! redirect back to the page, which then says thank you. Scripts only
//! enhance it: showing and hiding the questions that depend on others,
//! uploading photos as they are chosen, and picking places on a map.

use std::sync::Arc;

use anyhow::Error;
use common::{
    message::{
        form::{
            Answers, BooleanStyle, ChoiceWidget, FieldError, FieldKind, FormDefinition, FormField,
            FormLookup, FormSubmission, RawAnswers, RememberedRequest, SubmitOutcome, latest_date,
        },
        location::{LocationInfo, Point},
        user::BasicInfo,
    },
    service::request,
};
use jiff::{Timestamp, civil};
use poem::{
    FromRequest, IntoResponse, Response, handler,
    http::StatusCode,
    session::Session,
    web::{Form, Path, Query, RealIp, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::{
    component::{Component, Head, StandardLayout, html_escape},
    entity::{Entity, EntityKind},
    markdown::form_content_html,
    middleware::authorization::Authenticated,
};

/// What each of a form's controls is named by: its field's key behind this,
/// so that no key, however worded, can be taken for another control of the
/// page.
pub(super) const CONTROL_PREFIX: &str = "a.";

/// The session key naming the form whose page owes its visitor thanks: the
/// one they have just submitted.
const THANKS: &str = "hosted-form-thanks";

/// What the page has for its visitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// The form takes no submissions.
    Closed,
    /// They have just submitted it.
    Thanks,
    /// They have to sign in to submit it.
    SignInFirst,
    Open,
}

#[derive(Debug, Default, Deserialize)]
struct PageQuery {
    /// Where to send someone once they have submitted the form, in place of
    /// the form's own `next_url`: a path on this site.
    next: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "hosted_form/page.stpl.html")]
struct FormPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<FormPageContent> as Component>::Prepared,
    title: String,
}

#[derive(TemplateOnce)]
#[template(path = "hosted_form/content.stpl.html")]
struct FormPageContent {
    form: FormDefinition,
    state: State,
    user: Option<Arc<BasicInfo>>,
    /// What each field's controls show, by field key.
    values: RawAnswers,
    errors: Vec<FieldError>,
    /// Whether each field is shown, in order.
    shown: Vec<bool>,
    /// The page's own path.
    path: String,
    next: Option<String>,
    /// Where a map for picking a place starts out.
    center: Option<Point>,
    /// The latest date a date that may not be in the future may be.
    today: civil::Date,
}

impl FormPageContent {
    fn field(&self, index: usize) -> FieldControl<'_> {
        let field = &self.form.fields[index];

        FieldControl {
            form_id: self.form.form_id,
            index,
            field,
            values: self
                .values
                .get(field.key.as_str())
                .map(Vec::as_slice)
                .unwrap_or_default(),
            error: self
                .errors
                .iter()
                .find(|error| error.key == field.key)
                .map(|error| error.message.as_str()),
            shown: self.shown[index],
            condition_name: field
                .visible_if
                .as_ref()
                .map(|condition| control_name(&condition.field)),
            center: self.center,
            today: self.today,
        }
    }

    fn intro_html(&self) -> String {
        form_content_html(&self.form.intro)
    }

    fn aside_html(&self) -> String {
        form_content_html(&self.form.aside)
    }

    fn map_intro_html(&self) -> String {
        self.form
            .map
            .as_ref()
            .map(|map| form_content_html(&map.intro))
            .unwrap_or_default()
    }

    /// Where someone goes on from the thanks: where they were asked to go,
    /// else where the form sends people, if anywhere.
    fn onward(&self) -> Option<&str> {
        self.next
            .as_deref()
            .or_else(|| super::link(&self.form.next_url))
    }

    fn login_url(&self) -> String {
        format!("/login?next={}", urlencoding::encode(&self.path))
    }

    fn signup_url(&self) -> String {
        format!("/signup?next={}", urlencoding::encode(&self.path))
    }
}

/// One field of a form, as its page draws it.
#[derive(TemplateOnce)]
#[template(path = "hosted_form/field.stpl.html")]
struct FieldControl<'a> {
    form_id: Ulid,
    index: usize,
    field: &'a FormField,
    /// What its controls show.
    values: &'a [String],
    error: Option<&'a str>,
    shown: bool,
    /// The name of the controls whose answer shows this field, if one does.
    condition_name: Option<String>,
    center: Option<Point>,
    today: civil::Date,
}

impl FieldControl<'_> {
    fn id(&self) -> String {
        field_id(self.index)
    }

    fn name(&self) -> String {
        control_name(&self.field.key)
    }

    fn value(&self) -> &str {
        self.values.first().map(String::as_str).unwrap_or("")
    }

    fn has(&self, value: &str) -> bool {
        self.values.iter().any(|sent| sent == value)
    }

    fn help_html(&self) -> String {
        form_content_html(&self.field.help)
    }

    /// Whether the field's controls are a group, labelled by a legend.
    fn grouped(&self) -> bool {
        match &self.field.kind {
            FieldKind::Choice { widget, .. } => *widget == ChoiceWidget::Buttons,
            FieldKind::Boolean { style } => *style != BooleanStyle::Checkbox,
            FieldKind::Photo { .. } => true,
            _ => false,
        }
    }

    /// Whether the field is one box, ticked or not, labelled by its
    /// question.
    fn is_checkbox(&self) -> bool {
        matches!(
            self.field.kind,
            FieldKind::Boolean {
                style: BooleanStyle::Checkbox
            }
        )
    }

    /// The attributes a control of the field carries: its name, whether it
    /// must be answered, and what describes it. `own_id` is whether this
    /// control is the one the field's label is for.
    fn attributes(&self, own_id: bool, required: bool) -> String {
        let mut attributes = format!(r#"name="{}""#, html_escape(&self.name()));

        if own_id {
            attributes.push_str(&format!(r#" id="{}""#, self.id()));
        }

        if required {
            attributes.push_str(" required");
        }

        let mut described_by = Vec::new();
        if self.error.is_some() {
            attributes.push_str(r#" aria-invalid="true""#);
            described_by.push(format!("{}-error", self.id()));
        }
        if !self.field.help.trim().is_empty() {
            described_by.push(format!("{}-help", self.id()));
        }
        if !described_by.is_empty() {
            attributes.push_str(&format!(
                r#" aria-describedby="{}""#,
                described_by.join(" ")
            ));
        }

        attributes
    }

    /// Where a photo field's upload control sends its photos.
    fn photo_url(&self) -> String {
        format!(
            "/-form/{}/photo?field={}",
            self.form_id,
            urlencoding::encode(self.field.key.as_str())
        )
    }

    /// The times a time field offers as a list, where it offers them so.
    fn offered_times(&self) -> Vec<String> {
        let FieldKind::Time {
            min,
            max,
            step_minutes: Some(step),
            ..
        } = &self.field.kind
        else {
            return Vec::new();
        };

        let step = i64::from((*step).max(1));
        let first = min.unwrap_or(civil::Time::midnight());
        let last = max.unwrap_or(civil::Time::MAX);
        let minutes = |time: civil::Time| i64::from(time.hour()) * 60 + i64::from(time.minute());

        (minutes(first)..=minutes(last))
            .step_by(step as usize)
            .map(|minute| format!("{:02}:{:02}", minute / 60, minute % 60))
            .collect()
    }
}

/// The id of the field at `index` on its form's page, which its parts'
/// ids begin with.
pub(super) fn field_id(index: usize) -> String {
    format!("field-{index}")
}

/// The name of the controls answering the field `key`.
pub(super) fn control_name(key: &impl std::fmt::Display) -> String {
    format!("{CONTROL_PREFIX}{key}")
}

/// What a form sent, as answers by field key: its controls' values, and
/// nothing of the page's other controls.
pub(super) fn sent_answers(sent: &[(String, String)]) -> RawAnswers {
    let mut answers = RawAnswers::new();

    for (name, value) in sent {
        if let Some(key) = name.strip_prefix(CONTROL_PREFIX) {
            answers
                .entry(key.to_string())
                .or_default()
                .push(value.clone());
        }
    }

    answers
}

/// Whether each of the form's fields is shown, given what its controls
/// show: as [`FormField::shown`] decides from answers, and as the page's
/// script decides as the form is filled in. A field shown by another is
/// hidden while that one is.
fn visibility(form: &FormDefinition, values: &RawAnswers) -> Vec<bool> {
    let mut shown: Vec<bool> = Vec::with_capacity(form.fields.len());

    for field in &form.fields {
        let visible = match &field.visible_if {
            None => true,
            Some(condition) => form.fields[..shown.len()]
                .iter()
                .position(|earlier| earlier.key == condition.field)
                .is_some_and(|earlier| {
                    shown[earlier]
                        && answered_with(&form.fields[earlier], values)
                            .iter()
                            .any(|value| value.trim() == condition.equals)
                }),
        };

        shown.push(visible);
    }

    shown
}

/// What a field's controls answer, as far as showing another field goes:
/// only a choice, a text or a yes-or-no question can show one, and a box
/// left unticked answers no.
fn answered_with(field: &FormField, values: &RawAnswers) -> Vec<String> {
    let sent = values.get(field.key.as_str()).cloned().unwrap_or_default();

    match &field.kind {
        FieldKind::Boolean {
            style: BooleanStyle::Checkbox,
        } if sent.iter().all(|value| value.is_empty()) => vec!["false".to_string()],
        FieldKind::Choice { .. } | FieldKind::Text { .. } | FieldKind::Boolean { .. } => sent,
        _ => Vec::new(),
    }
}

/// What a form's controls show before anything is sent: the answers the
/// person gave last time to the questions the form remembers, and each
/// other question's starting answer.
async fn starting_values(form: &FormDefinition, user: Option<&BasicInfo>) -> RawAnswers {
    let remembered: Answers = match user {
        Some(user) if form.fields.iter().any(|field| field.remember) => request(
            "user.forms.remembered",
            RememberedRequest {
                form_id: form.form_id,
                user_id: user.user_id,
            },
        )
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Couldn't recall {}'s answers: {e:#}", user.user_id);
            Default::default()
        }),
        _ => Default::default(),
    };

    let mut values = RawAnswers::new();

    for field in form.fields.iter().filter(|field| field.kind.asks()) {
        let starting = match remembered.get(&field.key) {
            Some(answer) => field.control_values(answer),
            None => field.default.iter().cloned().collect(),
        };

        if !starting.is_empty() {
            values.insert(field.key.to_string(), starting);
        }
    }

    values
}

/// Where a map for picking a place starts out, for someone at `address`: the
/// place the address is looked up to be.
async fn center(form: &FormDefinition, address: Option<std::net::IpAddr>) -> Option<Point> {
    let asks_place = form
        .fields
        .iter()
        .any(|field| field.kind == FieldKind::Location);

    if !asks_place {
        return None;
    }

    let location: LocationInfo = request("location.ip", address?.to_string()).await.ok()?;
    location.place()
}

/// The path of the form's page.
pub(super) async fn page_path(form_id: Ulid) -> Result<String, Error> {
    Ok(match Entity::page_of(EntityKind::Form, form_id).await? {
        Some(page) => page.canonical_path(),
        None => format!("/form/{form_id}"),
    })
}

/// Everything the page draws, drawn.
#[allow(clippy::too_many_arguments)]
async fn render(
    form: FormDefinition,
    path: String,
    user: Option<Arc<BasicInfo>>,
    thanks: bool,
    values: RawAnswers,
    errors: Vec<FieldError>,
    next: Option<String>,
    address: Option<std::net::IpAddr>,
) -> Result<Response, Error> {
    let state = if form.closed {
        State::Closed
    } else if thanks {
        State::Thanks
    } else if form.sign_in_required && user.is_none() {
        State::SignInFirst
    } else {
        State::Open
    };

    let status = if errors.is_empty() {
        StatusCode::OK
    } else {
        StatusCode::UNPROCESSABLE_ENTITY
    };

    let uses_maps = form.map.is_some()
        || form
            .fields
            .iter()
            .any(|field| field.kind == FieldKind::Location);
    let title = form.name.clone();

    let content = FormPageContent {
        shown: visibility(&form, &values),
        center: center(&form, address).await,
        today: latest_date(Timestamp::now()),
        form,
        state,
        user: user.clone(),
        values,
        errors,
        path: path.clone(),
        next,
    };

    let mut head = Head::default();
    if uses_maps {
        head = head.with_maplibre();
    }

    let page = FormPage {
        head: head.prepare(None).await?,
        layout: StandardLayout::new(path, content).prepare(user).await?,
        title,
    };

    Ok(Response::builder()
        .status(status)
        .content_type("text/html; charset=utf-8")
        .body(page.render_once()?))
}

/// The page of the form `form_id`, which its entity serves at `path`.
pub async fn entity_response(
    form_id: Ulid,
    path: String,
    user: Option<Arc<BasicInfo>>,
    session: &Session,
    req: &poem::Request,
) -> poem::Result<Response> {
    let Some(form) = super::load(FormLookup::Id(form_id)).await? else {
        return Err(poem::Error::from_status(StatusCode::NOT_FOUND));
    };

    let query = Query::<PageQuery>::from_request_without_body(req)
        .await
        .map(|Query(query)| query)
        .unwrap_or_default();
    let next = query
        .next
        .as_deref()
        .and_then(super::site_path)
        .map(str::to_string);
    let RealIp(address) = RealIp::from_request_without_body(req).await?;

    let thanks = session.get::<String>(THANKS) == Some(form_id.to_string());
    if thanks {
        session.remove(THANKS);
    }

    let values = if thanks {
        RawAnswers::new()
    } else {
        starting_values(&form, user.as_deref()).await
    };

    Ok(render(form, path, user, thanks, values, Vec::new(), next, address).await?)
}

/// POST `/-form/:form_id`: submit the form. Refused, the page again with
/// why; saved, on to where the submission was to lead, or back to the page
/// to be thanked.
#[handler]
pub async fn submit(
    Authenticated(user): Authenticated,
    RealIp(address): RealIp,
    session: &Session,
    Path(form_id): Path<Ulid>,
    Form(sent): Form<Vec<(String, String)>>,
) -> Result<Response, Error> {
    let Some(form) = super::load(FormLookup::Id(form_id)).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let path = page_path(form_id).await?;
    let answers = sent_answers(&sent);
    let next = sent
        .iter()
        .find(|(name, _)| name == "next")
        .and_then(|(_, next)| super::site_path(next))
        .map(str::to_string);

    let unhosted = super::photo::unhosted(&form, &answers);
    let outcome = if unhosted.is_empty() {
        request(
            "user.forms.submit",
            FormSubmission {
                form_id,
                submitter: user.as_ref().map(|user| user.user_id),
                answers: answers.clone(),
                ip_address: address.map(|address| address.to_string()),
            },
        )
        .await?
    } else {
        SubmitOutcome::Refused(unhosted)
    };

    match outcome {
        SubmitOutcome::Saved { .. } => match next {
            Some(next) => Ok(Redirect::see_other(next).into_response()),
            None => {
                session.set(THANKS, form_id.to_string());
                Ok(Redirect::see_other(path).into_response())
            }
        },
        SubmitOutcome::Refused(errors) => {
            render(form, path, user, false, answers, errors, next, address).await
        }
        SubmitOutcome::NoSuchForm => Ok(StatusCode::NOT_FOUND.into_response()),
        // The form closed since the page was drawn, or the session the page
        // was drawn for ended: the page drawn again says so.
        SubmitOutcome::Closed => {
            let form = FormDefinition {
                closed: true,
                ..form
            };
            render(form, path, user, false, answers, Vec::new(), next, address).await
        }
        SubmitOutcome::SignInRequired => {
            render(form, path, None, false, answers, Vec::new(), next, address).await
        }
    }
}

#[cfg(test)]
mod tests;
