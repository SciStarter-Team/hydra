//! Public SciStarter Ambassador application (`/ambassadors-apply`).
//!
//! Ported from orrery's `ambassador_apply` / `ambassador_application_save` /
//! `ambassador_application_draft`. Gated behind the Foundations badge and the
//! Champion tutorial. The form prefills from the user's saved application and
//! profile; a lax draft autosave writes with `draft = true`, and final submit
//! validates (required fields, email, age >= 18, signature date) then calls
//! `user.ambassador.submit` (which stamps `submitted` and tags the user).
//!
//! Applications open periodically; [`APPLICATIONS_OPEN`] gates the submit
//! control (the champion page copy mirrors this). Flip it when applications
//! reopen.

use std::sync::Arc;

use anyhow::Error;
use common::message::dashboard::{
    AmbassadorApplication, AmbassadorApplicationForm, AmbassadorDirectoryEntry,
    AmbassadorDirectoryOrder, AmbassadorDirectoryQuery, AmbassadorDirectoryResponse,
    AmbassadorMessageForm, AmbassadorProfile, UserProfile,
};
use common::message::user::{BasicInfo, WorkflowType};
use common::service::request;
use common::timestring::TimeString;
use poem::{
    IntoResponse, Response, handler,
    http::StatusCode,
    web::{Form, Path, Query, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::component::{Component, Head, StandardLayout};
use crate::middleware::authorization::Authenticated;
use crate::training::fetch_status;

const PATH: &str = "/ambassadors-apply";

/// Whether the Ambassador program is currently accepting applications. When
/// false the form still renders (and drafts save) but the submit control is
/// disabled, matching the champion page's "you'll be notified when applications
/// open" copy.
const APPLICATIONS_OPEN: bool = false;

/// The apply-form body, rendered inside `StandardLayout`.
#[derive(TemplateOnce, Default)]
#[template(path = "ambassador/apply.stpl.html")]
pub struct ApplyContent {
    pub logged_in: bool,
    pub foundations_done: bool,
    pub champion_done: bool,
    pub already_submitted: bool,
    pub applications_open: bool,
    pub error: Option<String>,
    pub login_next: String,
    // Values (prefill / re-render after a validation error)
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone: String,
    pub address1: String,
    pub address2: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub birthdate: String,
    pub tee_size: String,
    pub handles: String,
    pub self_description: String,
    pub experience: String,
    pub motivation: String,
    pub related: String,
    pub vision: String,
    pub community_access: String,
    pub introduction: String,
    pub introduction_other: String,
    pub questions: String,
    pub gender: String,
    pub ethnicity: String,
    pub activities: String,
    pub signature_name: String,
    pub signature_date: String,
    pub citscimonth: bool,
    pub background_check: bool,
    pub conduct: bool,
}

#[derive(TemplateOnce)]
#[template(path = "ambassador/page.stpl.html")]
struct ApplyPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<ApplyContent> as Component>::Prepared,
    title: String,
}

/// The public directory body, rendered inside `StandardLayout`.
#[derive(TemplateOnce, Default)]
#[template(path = "ambassador/directory.stpl.html")]
pub struct DirectoryContent {
    pub entries: Vec<AmbassadorDirectoryEntry>,
    pub name: String,
    pub near: String,
    pub page: i64,
    pub pages: i64,
    pub total: i64,
}

#[derive(TemplateOnce)]
#[template(path = "ambassador/page.stpl.html")]
struct DirectoryPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<DirectoryContent> as Component>::Prepared,
    title: String,
}

/// A single public ambassador profile body.
#[derive(TemplateOnce)]
#[template(path = "ambassador/profile.stpl.html")]
pub struct ProfileContent {
    pub profile: AmbassadorProfile,
    /// Whether the viewer is signed in (contactable only when so).
    pub logged_in: bool,
    /// Viewer's name, prefilled into the contact form.
    pub viewer_first: String,
    pub viewer_last: String,
}

#[derive(TemplateOnce)]
#[template(path = "ambassador/page.stpl.html")]
struct ProfilePage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<ProfileContent> as Component>::Prepared,
    title: String,
}

/// Posted apply form. Checkboxes deserialize to `Some` only when checked; every
/// other field defaults to empty so partial drafts parse.
#[derive(Debug, Default, Deserialize)]
struct ApplyForm {
    #[serde(default)]
    first_name: String,
    #[serde(default)]
    last_name: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    phone: String,
    #[serde(default)]
    address1: String,
    #[serde(default)]
    address2: String,
    #[serde(default)]
    city: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    zip: String,
    #[serde(default)]
    birthdate: String,
    #[serde(default)]
    tee_size: String,
    #[serde(default)]
    handles: String,
    #[serde(default)]
    self_description: String,
    #[serde(default)]
    experience: String,
    #[serde(default)]
    motivation: String,
    #[serde(default)]
    related: String,
    #[serde(default)]
    vision: String,
    #[serde(default)]
    community_access: String,
    #[serde(default)]
    introduction: String,
    #[serde(default)]
    introduction_other: String,
    #[serde(default)]
    questions: String,
    #[serde(default)]
    gender: String,
    #[serde(default)]
    ethnicity: String,
    #[serde(default)]
    activities: String,
    #[serde(default)]
    signature_name: String,
    #[serde(default)]
    signature_date: String,
    #[serde(default)]
    citscimonth: Option<String>,
    #[serde(default)]
    background_check: Option<String>,
    #[serde(default)]
    conduct: Option<String>,
}

fn nonempty(s: String) -> Option<String> {
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

impl ApplyForm {
    /// Convert into the wire form written by user-service. `user_id` identifies
    /// the applicant.
    fn to_wire(&self, user_id: ulid::Ulid) -> AmbassadorApplicationForm {
        AmbassadorApplicationForm {
            user_id,
            phone: self.phone.clone(),
            city: self.city.clone(),
            state: self.state.clone(),
            tee_size: self.tee_size.clone(),
            self_description: self.self_description.clone(),
            contact_email: self.email.clone(),
            handles: self.handles.clone(),
            experience: self.experience.clone(),
            motivation: self.motivation.clone(),
            related: self.related.clone(),
            vision: self.vision.clone(),
            community_access: self.community_access.clone(),
            introduction: self.introduction.clone(),
            introduction_other: self.introduction_other.clone(),
            questions: self.questions.clone(),
            signature_name: self.signature_name.clone(),
            gender: self.gender.clone(),
            ethnicity: self.ethnicity.clone(),
            activities: self.activities.clone(),
            citscimonth: self.citscimonth.is_some(),
            background_check: self.background_check.is_some(),
            conduct: self.conduct.is_some(),
            birthdate: nonempty(self.birthdate.clone()),
            signature_date: nonempty(self.signature_date.clone()),
            first_name: self.first_name.clone(),
            last_name: self.last_name.clone(),
            address1: self.address1.clone(),
            address2: self.address2.clone(),
            zip: self.zip.clone(),
        }
    }

    /// Build the content struct from posted values (used to re-render the form
    /// with an error banner, preserving what the applicant typed).
    fn into_content(self, gate: Gate, error: Option<String>) -> ApplyContent {
        ApplyContent {
            logged_in: gate.logged_in,
            foundations_done: gate.foundations_done,
            champion_done: gate.champion_done,
            already_submitted: gate.already_submitted,
            applications_open: APPLICATIONS_OPEN,
            error,
            login_next: PATH.to_string(),
            first_name: self.first_name,
            last_name: self.last_name,
            email: self.email,
            phone: self.phone,
            address1: self.address1,
            address2: self.address2,
            city: self.city,
            state: self.state,
            zip: self.zip,
            birthdate: self.birthdate,
            tee_size: self.tee_size,
            handles: self.handles,
            self_description: self.self_description,
            experience: self.experience,
            motivation: self.motivation,
            related: self.related,
            vision: self.vision,
            community_access: self.community_access,
            introduction: self.introduction,
            introduction_other: self.introduction_other,
            questions: self.questions,
            gender: self.gender,
            ethnicity: self.ethnicity,
            activities: self.activities,
            signature_name: self.signature_name,
            signature_date: self.signature_date,
            citscimonth: self.citscimonth.is_some(),
            background_check: self.background_check.is_some(),
            conduct: self.conduct.is_some(),
        }
    }
}

/// Gating state shared between the GET prefill and the POST re-render.
#[derive(Clone, Copy, Default)]
struct Gate {
    logged_in: bool,
    foundations_done: bool,
    champion_done: bool,
    already_submitted: bool,
}

async fn gate_for(user_info: &Option<Arc<BasicInfo>>) -> Gate {
    match user_info {
        None => Gate::default(),
        Some(user) => {
            let id = user.user_id.to_string();
            let foundations = fetch_status(WorkflowType::Foundations, &id).await;
            let champion = fetch_status(WorkflowType::Ambassador, &id).await;
            let submitted = request::<Option<AmbassadorApplication>>("user.ambassador.get", id)
                .await
                .ok()
                .flatten()
                .map(|a| !a.draft && a.submitted.is_some())
                .unwrap_or(false);
            Gate {
                logged_in: true,
                foundations_done: foundations.badge_granted,
                champion_done: champion.completed_steps.iter().any(|s| s == "tutorial"),
                already_submitted: submitted,
            }
        }
    }
}

async fn render(
    user_info: Option<Arc<BasicInfo>>,
    content: ApplyContent,
) -> Result<Response, Error> {
    let layout = StandardLayout::new(PATH.to_string(), content)
        .prepare(user_info)
        .await?;
    ApplyPage {
        head: Head::default().prepare(None).await?,
        layout,
        title: "Apply to be a SciStarter Ambassador".to_string(),
    }
    .into_response(None)
    .await
}

#[handler]
pub async fn index(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let gate = gate_for(&user_info).await;

    // Prefill from the saved application + profile (only when signed in).
    let mut content = ApplyContent {
        logged_in: gate.logged_in,
        foundations_done: gate.foundations_done,
        champion_done: gate.champion_done,
        already_submitted: gate.already_submitted,
        applications_open: APPLICATIONS_OPEN,
        login_next: PATH.to_string(),
        ..Default::default()
    };

    if let Some(user) = &user_info {
        let id = user.user_id.to_string();
        if let Ok(profile) = request::<UserProfile>("user.profile.get", id.clone()).await {
            content.first_name = profile.first_name;
            content.last_name = profile.last_name;
            content.email = profile.primary_email;
            content.zip = profile.zipcode.unwrap_or_default();
            content.birthdate = profile.birthday.clone().unwrap_or_default();
            // mailing_address is address1\naddress2\ncity\nstate\nzip
            if let Some(addr) = profile.mailing_address {
                let mut lines = addr.lines();
                content.address1 = lines.next().unwrap_or("").to_string();
                content.address2 = lines.next().unwrap_or("").to_string();
            }
        }
        if let Ok(Some(app)) =
            request::<Option<AmbassadorApplication>>("user.ambassador.get", id).await
        {
            if !app.contact_email.is_empty() {
                content.email = app.contact_email;
            }
            content.city = app.city;
            content.state = app.state;
            content.phone = app.phone;
            content.tee_size = app.tee_size;
            content.handles = app.handles;
            content.self_description = app.self_description;
            content.experience = app.experience;
            content.motivation = app.motivation;
            content.related = app.related;
            content.vision = app.vision;
            content.community_access = app.community_access;
            content.introduction = app.introduction;
            content.introduction_other = app.introduction_other;
            content.questions = app.questions;
            content.gender = app.gender;
            content.ethnicity = app.ethnicity;
            content.activities = app.activities;
            content.signature_name = app.signature_name;
            content.signature_date = app.signature_date.unwrap_or_default();
            content.citscimonth = app.citscimonth;
            content.background_check = app.background_check;
            content.conduct = app.conduct;
            if let Some(bd) = app.birthdate {
                content.birthdate = bd;
            }
        }
    }

    render(user_info, content).await
}

#[handler]
pub async fn submit(
    Authenticated(user_info): Authenticated,
    Form(form): Form<ApplyForm>,
) -> Result<Response, Error> {
    let Some(user) = user_info.clone() else {
        return Ok(Redirect::see_other("/login?next=/ambassadors-apply").into_response());
    };
    let gate = gate_for(&user_info).await;

    if !gate.foundations_done {
        return err(user_info, form, gate, "You must complete the Foundations of Citizen Science training before submitting your application.").await;
    }
    if !gate.champion_done {
        return err(user_info, form, gate, "You must complete the Citizen Science Champion training before submitting your application.").await;
    }
    if !APPLICATIONS_OPEN {
        return err(
            user_info,
            form,
            gate,
            "Ambassador applications are not open right now. You'll be notified when they reopen.",
        )
        .await;
    }

    // Required fields (mirrors orrery's _ambassador_apply_required_fields).
    let required = [
        ("birthdate", &form.birthdate),
        ("phone", &form.phone),
        ("city", &form.city),
        ("state", &form.state),
        ("t-shirt size", &form.tee_size),
        ("email", &form.email),
        ("experience", &form.experience),
        ("motivation", &form.motivation),
        ("vision", &form.vision),
        ("first name", &form.first_name),
        ("last name", &form.last_name),
        ("street address", &form.address1),
        ("ZIP", &form.zip),
    ];
    let missing: Vec<&str> = required
        .iter()
        .filter(|(_, v)| v.trim().is_empty())
        .map(|(label, _)| *label)
        .collect();
    if !missing.is_empty() {
        let msg = format!(
            "Please fill in the required fields: {}.",
            missing.join(", ")
        );
        return err(user_info, form, gate, &msg).await;
    }

    if !valid_email(&form.email) {
        return err(user_info, form, gate, "Please enter a valid email address.").await;
    }

    let today = TimeString::now();
    let today = today.get(0..10).unwrap_or("");
    match age_at_least_18(&form.birthdate, today) {
        Some(true) => {}
        Some(false) => {
            return err(
                user_info,
                form,
                gate,
                "You must be at least 18 years old to apply.",
            )
            .await;
        }
        None => {
            return err(user_info, form, gate, "Please enter a valid birthdate.").await;
        }
    }

    if !form.signature_date.trim().is_empty() {
        match form.signature_date.get(0..10) {
            Some(sig) if sig <= today => {}
            Some(_) => {
                return err(
                    user_info,
                    form,
                    gate,
                    "The signature date cannot be in the future.",
                )
                .await;
            }
            None => {
                return err(
                    user_info,
                    form,
                    gate,
                    "Please enter a valid signature date.",
                )
                .await;
            }
        }
    }

    let _: () = request("user.ambassador.submit", form.to_wire(user.user_id)).await?;
    Ok(Redirect::see_other(PATH).into_response())
}

/// Re-render the form after a validation failure.
async fn err(
    user_info: Option<Arc<BasicInfo>>,
    form: ApplyForm,
    gate: Gate,
    message: &str,
) -> Result<Response, Error> {
    let content = form.into_content(gate, Some(message.to_string()));
    render(user_info, content).await
}

#[handler]
pub async fn draft(
    Authenticated(user_info): Authenticated,
    Form(form): Form<ApplyForm>,
) -> Result<Response, Error> {
    let Some(user) = user_info else {
        return Ok(
            poem::web::Html("<span class=\"text-red-600\">Please log in.</span>").into_response(),
        );
    };
    let _: () = request("user.ambassador.save_draft", form.to_wire(user.user_id)).await?;
    Ok(poem::web::Html("<span class=\"text-green-600\">Draft saved.</span>").into_response())
}

/// Query params for the public directory.
#[derive(Debug, Default, Deserialize)]
struct DirectoryParams {
    #[serde(default)]
    name: String,
    #[serde(default)]
    near: String,
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    order: Option<String>,
}

#[handler]
pub async fn directory(
    Query(params): Query<DirectoryParams>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let order = match params.order.as_deref() {
        Some("time") => AmbassadorDirectoryOrder::Time,
        Some("name") => AmbassadorDirectoryOrder::Name,
        _ => AmbassadorDirectoryOrder::Default,
    };
    let resp: AmbassadorDirectoryResponse = request(
        "user.ambassador.directory",
        AmbassadorDirectoryQuery {
            name: params.name.clone(),
            near: params.near.clone(),
            page: params.page.unwrap_or(1),
            order,
        },
    )
    .await?;

    let content = DirectoryContent {
        entries: resp.entries,
        name: params.name,
        near: params.near,
        page: resp.page,
        pages: resp.pages,
        total: resp.total,
    };
    let layout = StandardLayout::new("/ambassadors".to_string(), content)
        .prepare(user_info)
        .await?;
    DirectoryPage {
        head: Head::default().with_maplibre().prepare(None).await?,
        layout,
        title: "SciStarter Ambassadors".to_string(),
    }
    .into_response(None)
    .await
}

#[handler]
pub async fn profile_page(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let not_found = || {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body("Ambassador not found")
    };
    let Ok(user_id) = Ulid::from_string(&id) else {
        return Ok(not_found());
    };
    let profile: Option<AmbassadorProfile> =
        request("user.ambassador.profile", user_id.to_string()).await?;
    let Some(profile) = profile else {
        return Ok(not_found());
    };

    // Viewer identity prefills the contact form (visitors must be signed in to
    // contact an ambassador).
    let (viewer_first, viewer_last) = match &user_info {
        Some(user) => request::<UserProfile>("user.profile.get", user.user_id.to_string())
            .await
            .map(|p| (p.first_name, p.last_name))
            .unwrap_or_default(),
        None => (String::new(), String::new()),
    };

    let content = ProfileContent {
        profile,
        logged_in: user_info.is_some(),
        viewer_first,
        viewer_last,
    };
    let layout = StandardLayout::new(format!("/ambassadors/{id}"), content)
        .prepare(user_info)
        .await?;
    ProfilePage {
        head: Head::default().prepare(None).await?,
        layout,
        title: "SciStarter Ambassador".to_string(),
    }
    .into_response(None)
    .await
}

/// Posted contact-an-ambassador form. Mirrors orrery's contact modal fields
/// (subject is retained but optional; orrery has it commented out of the UI).
#[derive(Debug, Default, Deserialize)]
struct ContactForm {
    #[serde(default)]
    first_name: String,
    #[serde(default)]
    last_name: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    dates: String,
    #[serde(default)]
    body: String,
}

/// Small HTML fragment swapped into the contact form's result slot.
fn contact_result(html: &str) -> Response {
    poem::web::Html(html.to_string()).into_response()
}

/// Queue a contact message to an ambassador (`POST /ambassadors/:id/contact`).
/// The message is held for staff moderation before delivery, so the visitor sees
/// a "received, pending review" confirmation. HTMX swaps the returned fragment
/// into the form's result slot.
#[handler]
pub async fn contact(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
    Form(form): Form<ContactForm>,
) -> Result<Response, Error> {
    let Some(user) = user_info else {
        return Ok(contact_result(
            "<p class=\"text-red-600\">Please log in to contact this ambassador.</p>",
        ));
    };
    let Ok(recipient_id) = Ulid::from_string(&id) else {
        return Ok(contact_result(
            "<p class=\"text-red-600\">This ambassador could not be found.</p>",
        ));
    };

    if form.first_name.trim().is_empty()
        || form.last_name.trim().is_empty()
        || form.body.trim().is_empty()
    {
        return Ok(contact_result(
            "<p class=\"text-red-600\">Please provide your name and a message.</p>",
        ));
    }

    let _: () = request(
        "user.ambassador.message_send",
        AmbassadorMessageForm {
            sender_id: user.user_id,
            recipient_id,
            first_name: form.first_name,
            last_name: form.last_name,
            subject: form.subject,
            dates: form.dates,
            body: form.body,
        },
    )
    .await?;

    Ok(contact_result(
        "<p class=\"text-green-700\">Your message has been received. \
         It will be reviewed by SciStarter staff before being forwarded to the ambassador.</p>",
    ))
}

fn valid_email(email: &str) -> bool {
    let mut parts = email.trim().splitn(2, '@');
    match (parts.next(), parts.next()) {
        (Some(local), Some(domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !domain.contains(char::is_whitespace)
        }
        _ => false,
    }
}

fn parse_ymd(s: &str) -> Option<(i32, u32, u32)> {
    let s = s.get(0..10)?;
    let mut it = s.split('-');
    let y = it.next()?.parse().ok()?;
    let m = it.next()?.parse().ok()?;
    let d = it.next()?.parse().ok()?;
    Some((y, m, d))
}

/// Whether someone born on `birthdate` is at least 18 as of `today` (both
/// `YYYY-MM-DD`). `None` if either date is unparseable.
fn age_at_least_18(birthdate: &str, today: &str) -> Option<bool> {
    let (by, bm, bd) = parse_ymd(birthdate)?;
    let (ty, tm, td) = parse_ymd(today)?;
    let mut age = ty - by;
    if (tm, td) < (bm, bd) {
        age -= 1;
    }
    Some(age >= 18)
}
