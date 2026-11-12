//! Staff's form builder, at `/-admin/forms`: every form, and an editor for
//! each, a card for each of its fields.
//!
//! The editor is one plain form, sent whole. Each field's card names its
//! controls `field.<token>.<property>`, the token telling the cards apart,
//! and the cards are read in the order they come ([`read_draft`]): moving a
//! card or taking it out is done on the page alone, and adding one fetches
//! a blank card for the kind of field chosen.
//!
//! Until cutover a copy of one of orrery's forms is kept as orrery has it
//! (`common::primary::CopyKeeper`), and user-service refuses to change or
//! delete it; its editor shows it without letting it be changed.

use std::{collections::HashMap, str::FromStr, sync::Arc};

use anyhow::Error;
use common::{
    message::{
        form::{
            BooleanStyle, ChoiceOption, ChoiceWidget, Condition, FieldKey, FieldKind,
            FormDefinition, FormDeleted, FormField, FormLookup, FormMap, FormProject,
            FormProjectRole, FormSaved, FormSummary,
        },
        user::{BasicInfo, UserByEmailRequest, UserByEmailResponse},
    },
    primary::CopyKeeper,
    service::{log_activity, request},
    shared::db,
    timestring::TimeString,
};
use jiff::civil;
use poem::{
    Endpoint, IntoResponse, Response, Route, get, handler,
    http::StatusCode,
    post,
    web::{Form, Path, Query, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::{
    component::{AdminLayout, Component},
    entity::{Entity, EntityKind},
    middleware::authorization::Authenticated,
};

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(list))
        .at("/new", get(new_form))
        .at("/field", get(new_field))
        .at("/:form_id", get(edit).post(save))
        .at("/:form_id/delete", post(delete))
}

// ==================== Kinds of field ====================

/// The kinds of field, as the builder names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Heading,
    Markdown,
    Image,
    Text,
    Number,
    Choice,
    Boolean,
    Date,
    Time,
    DateTime,
    Photo,
    Location,
}

impl Kind {
    pub const ALL: [Kind; 12] = [
        Kind::Text,
        Kind::Number,
        Kind::Choice,
        Kind::Boolean,
        Kind::Date,
        Kind::Time,
        Kind::DateTime,
        Kind::Photo,
        Kind::Location,
        Kind::Heading,
        Kind::Markdown,
        Kind::Image,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Heading => "heading",
            Kind::Markdown => "markdown",
            Kind::Image => "image",
            Kind::Text => "text",
            Kind::Number => "number",
            Kind::Choice => "choice",
            Kind::Boolean => "boolean",
            Kind::Date => "date",
            Kind::Time => "time",
            Kind::DateTime => "date_time",
            Kind::Photo => "photo",
            Kind::Location => "location",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Heading => "Heading",
            Kind::Markdown => "Text to read (Markdown)",
            Kind::Image => "Picture",
            Kind::Text => "Text answer",
            Kind::Number => "Number",
            Kind::Choice => "Choice",
            Kind::Boolean => "Yes or no",
            Kind::Date => "Date",
            Kind::Time => "Time of day",
            Kind::DateTime => "Date and time",
            Kind::Photo => "Photo",
            Kind::Location => "Place on a map",
        }
    }

    fn of(kind: &FieldKind) -> Kind {
        match kind {
            FieldKind::Heading => Kind::Heading,
            FieldKind::Markdown { .. } => Kind::Markdown,
            FieldKind::Image { .. } => Kind::Image,
            FieldKind::Text { .. } => Kind::Text,
            FieldKind::Number { .. } => Kind::Number,
            FieldKind::Choice { .. } => Kind::Choice,
            FieldKind::Boolean { .. } => Kind::Boolean,
            FieldKind::Date { .. } => Kind::Date,
            FieldKind::Time { .. } => Kind::Time,
            FieldKind::DateTime => Kind::DateTime,
            FieldKind::Photo { .. } => Kind::Photo,
            FieldKind::Location => Kind::Location,
        }
    }

    /// A field of this kind as a new card shows it.
    fn blank(self) -> FieldKind {
        match self {
            Kind::Heading => FieldKind::Heading,
            Kind::Markdown => FieldKind::Markdown {
                text: String::new(),
            },
            Kind::Image => FieldKind::Image { url: String::new() },
            Kind::Text => FieldKind::Text {
                multiline: false,
                placeholder: String::new(),
            },
            Kind::Number => FieldKind::Number {
                min: None,
                max: None,
                integer: false,
            },
            Kind::Choice => FieldKind::Choice {
                options: Vec::new(),
                multiple: false,
                widget: ChoiceWidget::Buttons,
            },
            Kind::Boolean => FieldKind::Boolean {
                style: BooleanStyle::YesNo,
            },
            Kind::Date => FieldKind::Date {
                not_in_future: false,
            },
            Kind::Time => FieldKind::Time {
                min: None,
                max: None,
                step_minutes: None,
                not_before: None,
            },
            Kind::DateTime => FieldKind::DateTime,
            Kind::Photo => FieldKind::Photo { multiple: false },
            Kind::Location => FieldKind::Location,
        }
    }
}

impl FromStr for Kind {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Kind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == text)
            .ok_or_else(|| format!("{text:?} is no kind of field"))
    }
}

pub(super) const WIDGETS: [(ChoiceWidget, &str, &str); 2] = [
    (ChoiceWidget::Buttons, "buttons", "Buttons to pick from"),
    (ChoiceWidget::Select, "select", "A drop-down list"),
];

pub(super) const STYLES: [(BooleanStyle, &str, &str); 3] = [
    (BooleanStyle::YesNo, "yes_no", "“Yes” and “No”"),
    (BooleanStyle::TrueFalse, "true_false", "“True” and “False”"),
    (BooleanStyle::Checkbox, "checkbox", "One box to tick"),
];

pub(super) const ROLES: [(FormProjectRole, &str, &str); 2] = [
    (
        FormProjectRole::Main,
        "main",
        "The project’s main form, where taking part leads",
    ),
    (
        FormProjectRole::Extra,
        "extra",
        "Another form the project offers",
    ),
];

/// What `table` names `name`.
fn named<T: Copy>(table: &[(T, &str, &str)], name: &str) -> Option<T> {
    table
        .iter()
        .find(|(_, each, _)| *each == name)
        .map(|(value, _, _)| *value)
}

// ==================== Reading the editor ====================

/// The editor's controls, as sent.
struct Sent<'a>(&'a [(String, String)]);

impl Sent<'_> {
    /// The first value sent for `name`, as sent.
    fn raw(&self, name: &str) -> &str {
        self.0
            .iter()
            .find(|(each, _)| each == name)
            .map_or("", |(_, value)| value.as_str())
    }

    /// The first value sent for `name`, trimmed.
    fn one(&self, name: &str) -> &str {
        self.raw(name).trim()
    }

    /// The first value sent for `name`, unless it is blank.
    fn given(&self, name: &str) -> Option<String> {
        Some(self.one(name))
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }

    /// Whether the box `name` is ticked.
    fn ticked(&self, name: &str) -> bool {
        self.0.iter().any(|(each, _)| each == name)
    }

    /// The tokens of the field cards sent, in the order they come.
    fn cards(&self) -> Vec<&str> {
        let mut tokens: Vec<&str> = Vec::new();

        for (name, _) in self.0 {
            let Some(token) = name
                .strip_prefix("field.")
                .and_then(|rest| rest.split_once('.'))
                .map(|(token, _)| token)
            else {
                continue;
            };

            if !tokens.contains(&token) {
                tokens.push(token);
            }
        }

        tokens
    }
}

/// The control of the card `token` for its field's `property`.
pub(super) fn card_control(token: &str, property: &str) -> String {
    format!("field.{token}.{property}")
}

/// The options of a choice, written one to a line: `value | label`, or the
/// label alone where it is the value too.
pub(super) fn options_text(options: &[ChoiceOption]) -> String {
    options
        .iter()
        .map(|option| {
            if option.value == option.label {
                option.label.clone()
            } else {
                format!("{} | {}", option.value, option.label)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn read_options(text: &str) -> Vec<ChoiceOption> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| match line.split_once('|') {
            Some((value, label)) => ChoiceOption {
                value: value.trim().to_string(),
                label: label.trim().to_string(),
            },
            None => ChoiceOption {
                value: line.to_string(),
                label: line.to_string(),
            },
        })
        .collect()
}

/// A key made from `label`: its words, lowercased and joined by dashes.
fn key_from(label: &str) -> String {
    crate::slug::slugify(label)
}

/// The field the card `token` describes, and why any of it was refused.
fn read_field(sent: &Sent, token: &str, errors: &mut Vec<String>) -> Option<FormField> {
    let control = |property: &str| card_control(token, property);

    let kind: Kind = match sent.one(&control("kind")).parse() {
        Ok(kind) => kind,
        Err(e) => {
            errors.push(e);
            return None;
        }
    };

    let label = sent.one(&control("label")).to_string();
    let key = match sent.one(&control("key")) {
        "" => Some(key_from(&label))
            .filter(|key| !key.is_empty())
            .unwrap_or_else(|| token.to_ascii_lowercase()),
        key => key.to_string(),
    };

    // What the refusals call the field.
    let called = if label.is_empty() {
        key.clone()
    } else {
        format!("“{label}”")
    };

    let mut number = |property: &str, what: &str| -> Option<f64> {
        let text = sent.one(&control(property));
        match text {
            "" => None,
            text => match text.parse::<f64>() {
                Ok(n) if n.is_finite() => Some(n),
                _ => {
                    errors.push(format!("{called}: {what} must be a number, not {text:?}."));
                    None
                }
            },
        }
    };

    let kind = match kind {
        Kind::Heading => FieldKind::Heading,
        Kind::Markdown => FieldKind::Markdown {
            text: sent.raw(&control("text")).trim_end().to_string(),
        },
        Kind::Image => FieldKind::Image {
            url: sent.one(&control("url")).to_string(),
        },
        Kind::Text => FieldKind::Text {
            multiline: sent.ticked(&control("multiline")),
            placeholder: sent.one(&control("placeholder")).to_string(),
        },
        Kind::Number => FieldKind::Number {
            min: number("min", "the least it may be"),
            max: number("max", "the most it may be"),
            integer: sent.ticked(&control("integer")),
        },
        Kind::Choice => FieldKind::Choice {
            options: read_options(sent.raw(&control("options"))),
            multiple: sent.ticked(&control("multiple")),
            widget: named(&WIDGETS, sent.one(&control("widget"))).unwrap_or(ChoiceWidget::Buttons),
        },
        Kind::Boolean => FieldKind::Boolean {
            style: named(&STYLES, sent.one(&control("style"))).unwrap_or(BooleanStyle::YesNo),
        },
        Kind::Date => FieldKind::Date {
            not_in_future: sent.ticked(&control("not_in_future")),
        },
        Kind::Time => {
            let mut time = |property: &str, what: &str| -> Option<civil::Time> {
                let text = sent.one(&control(property));
                match text {
                    "" => None,
                    text => match text.parse::<civil::Time>() {
                        Ok(time) => Some(time),
                        Err(_) => {
                            errors.push(format!(
                                "{called}: {what} must be a time of day, not {text:?}."
                            ));
                            None
                        }
                    },
                }
            };
            let min = time("min", "the earliest it may be");
            let max = time("max", "the latest it may be");

            let step = sent.one(&control("step_minutes"));
            let step_minutes = match step {
                "" => None,
                step => match step.parse::<u16>() {
                    Ok(step) if step > 0 => Some(step),
                    _ => {
                        errors.push(format!("{called}: the minutes between times must be a whole number, not {step:?}."));
                        None
                    }
                },
            };

            FieldKind::Time {
                min,
                max,
                step_minutes,
                not_before: sent.given(&control("not_before")).map(FieldKey),
            }
        }
        Kind::DateTime => FieldKind::DateTime,
        Kind::Photo => FieldKind::Photo {
            multiple: sent.ticked(&control("multiple")),
        },
        Kind::Location => FieldKind::Location,
    };

    if let FieldKind::Choice { options, .. } = &kind
        && options.is_empty()
    {
        errors.push(format!("{called} offers nothing to choose."));
    }

    let visible_if = sent
        .given(&control("show_if_field"))
        .map(|field| Condition {
            field: FieldKey(field),
            equals: sent.one(&control("show_if_equals")).to_string(),
        });

    Some(FormField {
        key: FieldKey(key),
        label,
        help: sent.raw(&control("help")).trim_end().to_string(),
        required: sent.ticked(&control("required")),
        visible_if,
        remember: sent.ticked(&control("remember")),
        default: sent.given(&control("default")),
        kind,
    })
}

/// What the editor sent: the form it describes, but for its project and
/// owners, which are looked up ([`resolve`]); and why any of it was refused.
struct Draft {
    form: FormDefinition,
    /// The project box: a project's id, slug or address.
    project: String,
    /// The role the form is to have in its project, if it has one.
    role: FormProjectRole,
    /// The owners box: their email addresses, one to a line.
    owners: String,
    errors: Vec<String>,
}

/// Whether `slug` is fit to be the last part of a form's address.
fn fit_slug(slug: &str) -> bool {
    slug.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The form the editor sent for `form_id`, kept as `kept` until now: what
/// the editor cannot change (its orrery id, when it was made) is `kept`'s.
fn read_draft(pairs: &[(String, String)], form_id: Ulid, kept: Option<&FormDefinition>) -> Draft {
    let sent = Sent(pairs);
    let mut errors = Vec::new();

    let name = sent.one("name").to_string();
    if name.is_empty() {
        errors.push("A form needs a name.".to_string());
    }

    let slug = sent.given("slug");
    if let Some(slug) = &slug
        && !fit_slug(slug)
    {
        errors.push(format!(
            "The address {slug:?} may have only letters, numbers, dashes and underscores."
        ));
    }

    let fields = sent
        .cards()
        .into_iter()
        .filter_map(|token| read_field(&sent, token, &mut errors))
        .collect();

    let map = sent.ticked("map").then(|| FormMap {
        intro: sent.raw("map_intro").trim_end().to_string(),
        popup_fields: sent
            .raw("map_popup_fields")
            .split([',', '\n'])
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(FieldKey::from)
            .collect(),
    });

    let now = TimeString::now();
    let form = FormDefinition {
        form_id,
        orrery_id: kept.and_then(|kept| kept.orrery_id),
        slug,
        name,
        intro: sent.raw("intro").trim_end().to_string(),
        aside: sent.raw("aside").trim_end().to_string(),
        image: sent.one("image").to_string(),
        fields,
        closed: sent.ticked("closed"),
        sign_in_required: sent.ticked("sign_in_required"),
        anonymized: sent.ticked("anonymized"),
        credits_participation: sent.ticked("credits_participation"),
        repetitions_field: sent.given("repetitions_field").map(FieldKey),
        next_url: sent.one("next_url").to_string(),
        confirmation_message: sent.raw("confirmation_message").trim().to_string(),
        first_submission_email: sent.given("first_submission_email"),
        project: None,
        owners: Vec::new(),
        map,
        created: kept.map_or_else(|| now.clone(), |kept| kept.created.clone()),
        updated: now,
    };

    if let Some(next) = sent.given("next_url")
        && super::link(&next).is_none()
    {
        errors.push(format!(
            "Where to go next must be a path on this site or an address on the web, not {next:?}."
        ));
    }

    Draft {
        form,
        project: sent.one("project").to_string(),
        role: named(&ROLES, sent.one("project_role")).unwrap_or(FormProjectRole::Extra),
        owners: sent.raw("owners").to_string(),
        errors,
    }
}

/// The project `text` names: its id, its page's slug, or its page's
/// address.
async fn project_named(text: &str) -> Result<Option<Ulid>, Error> {
    let text = text.trim().trim_end_matches('/');
    let slug = text
        .rsplit_once("/projects/")
        .map_or(text, |(_, slug)| slug);

    if let Ok(project_id) = slug.parse::<Ulid>() {
        return Ok(Entity::page_of(EntityKind::Project, project_id)
            .await?
            .map(|_| project_id));
    }

    let target = sqlx::query_scalar!(
        r#"SELECT "target" FROM "entities" WHERE "kind" = ? AND LOWER("slug") = LOWER(?)"#,
        EntityKind::Project,
        slug
    )
    .fetch_optional(db())
    .await?;

    Ok(target.and_then(|target| target.parse().ok()))
}

/// Look up the draft's project and owners, refusing any that cannot be
/// found.
async fn resolve(draft: &mut Draft) -> Result<(), Error> {
    let role = draft.role;

    if !draft.project.trim().is_empty() {
        match project_named(&draft.project).await? {
            Some(project_id) => {
                draft.form.project = Some(FormProject {
                    project_id: project_id.to_string(),
                    role,
                });
            }
            None => draft
                .errors
                .push(format!("There is no project {:?}.", draft.project.trim())),
        }
    }

    for email in draft
        .owners
        .split([',', '\n', ' '])
        .map(str::trim)
        .filter(|email| !email.is_empty())
    {
        let found: UserByEmailResponse = request(
            "user.search.by_email",
            UserByEmailRequest {
                email: email.to_string(),
            },
        )
        .await?;

        match found
            .user_id
            .filter(|_| found.found)
            .and_then(|id| id.parse().ok())
        {
            Some(user_id) if !draft.form.owners.contains(&user_id) => {
                draft.form.owners.push(user_id)
            }
            Some(_) => {}
            None => draft
                .errors
                .push(format!("Nobody has the address {email:?}.")),
        }
    }

    Ok(())
}

// ==================== Showing the editor ====================

/// One field, as its card in the editor shows it.
#[derive(TemplateOnce)]
#[template(path = "admin/forms_field_card.stpl.html")]
pub(super) struct FieldCard {
    pub token: String,
    pub field: FormField,
}

impl FieldCard {
    fn kind(&self) -> Kind {
        Kind::of(&self.field.kind)
    }

    fn control(&self, property: &str) -> String {
        card_control(&self.token, property)
    }

    fn id(&self, property: &str) -> String {
        format!("fb-{}-{property}", self.token)
    }

    fn number(n: &Option<f64>) -> String {
        n.map(|n| n.to_string()).unwrap_or_default()
    }

    fn time(time: &Option<civil::Time>) -> String {
        time.map(|time| time.strftime("%H:%M").to_string())
            .unwrap_or_default()
    }

    fn show_if_field(&self) -> &str {
        self.field
            .visible_if
            .as_ref()
            .map_or("", |condition| condition.field.as_str())
    }

    fn show_if_equals(&self) -> &str {
        self.field
            .visible_if
            .as_ref()
            .map_or("", |condition| condition.equals.as_str())
    }

    /// Whether the field asks something, rather than showing something.
    fn asks(&self) -> bool {
        self.field.kind.asks()
    }
}

#[derive(TemplateOnce)]
#[template(path = "admin/forms_editor.stpl.html")]
struct EditorPanel {
    form: FormDefinition,
    /// The role the project box gives the form.
    role: FormProjectRole,
    is_new: bool,
    /// The form is a copy orrery keeps, so cannot be changed here.
    read_only: bool,
    project: String,
    owners: String,
    errors: Vec<String>,
    saved: bool,
    /// The form's page, once it has one.
    page_path: Option<String>,
    cards: Vec<FieldCard>,
}

impl EditorPanel {
    fn popup_fields(&self) -> String {
        self.form.map.as_ref().map_or_else(String::new, |map| {
            map.popup_fields
                .iter()
                .map(FieldKey::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
    }
}

/// The editor for `form`, showing its project and owners as people know
/// them.
async fn editor(form: FormDefinition, is_new: bool, saved: bool) -> Result<EditorPanel, Error> {
    let project = match &form.project {
        Some(project) => match project.project_id.parse::<Ulid>() {
            Ok(project_id) => Entity::page_of(EntityKind::Project, project_id)
                .await?
                .map_or_else(
                    || project.project_id.clone(),
                    |page| page.slug().to_string(),
                ),
            Err(_) => project.project_id.clone(),
        },
        None => String::new(),
    };

    let emails: HashMap<Ulid, String> =
        crate::admin::autocomplete::resolve_user_names(form.owners.clone())
            .await?
            .into_iter()
            .collect();
    let owners = form
        .owners
        .iter()
        .map(|owner| {
            emails
                .get(owner)
                .cloned()
                .unwrap_or_else(|| owner.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n");

    let role = form
        .project
        .as_ref()
        .map_or(FormProjectRole::Extra, |project| project.role);

    drafted(form, is_new, project, role, owners, Vec::new(), saved).await
}

/// The editor showing `form` as drafted, with the refusals `errors`.
#[allow(clippy::too_many_arguments)]
async fn drafted(
    form: FormDefinition,
    is_new: bool,
    project: String,
    role: FormProjectRole,
    owners: String,
    errors: Vec<String>,
    saved: bool,
) -> Result<EditorPanel, Error> {
    let page_path = if is_new {
        None
    } else {
        Entity::page_of(EntityKind::Form, form.form_id)
            .await?
            .map(|page| page.canonical_path())
    };

    let cards = form
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| FieldCard {
            token: format!("f{index}"),
            field: field.clone(),
        })
        .collect();

    Ok(EditorPanel {
        read_only: CopyKeeper::NOW.orrery_owns(form.orrery_id),
        form,
        role,
        is_new,
        project,
        owners,
        errors,
        saved,
        page_path,
        cards,
    })
}

async fn page<C: TemplateOnce>(
    title: &str,
    content: C,
    user: Option<Arc<BasicInfo>>,
) -> Result<Response, Error> {
    let mut layout = AdminLayout::new(title, content)
        .with_sidebar_active("forms")
        .with_breadcrumb("Forms", Some("/-admin/forms"));
    if title != "Forms" {
        layout = layout.with_breadcrumb(title, None);
    }

    let prepared = layout.prepare(user).await?;

    Ok(Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(prepared.render_once()?))
}

// ==================== Handlers ====================

#[derive(TemplateOnce)]
#[template(path = "admin/forms_list.stpl.html")]
struct FormList {
    forms: Vec<super::FormLink>,
}

/// GET `/-admin/forms`: every form.
#[handler]
async fn list(Authenticated(user): Authenticated) -> Result<Response, Error> {
    let mut summaries: Vec<FormSummary> = request("user.forms.list", ()).await?;
    summaries.sort_by_key(|form| form.name.to_lowercase());

    let forms = super::with_pages(summaries).await?;

    page("Forms", FormList { forms }, user).await
}

/// GET `/-admin/forms/new`: the editor for a new form.
#[handler]
async fn new_form(Authenticated(user): Authenticated) -> Result<Response, Error> {
    let now = TimeString::now();
    let form = FormDefinition {
        form_id: Ulid::new(),
        orrery_id: None,
        slug: None,
        name: String::new(),
        intro: String::new(),
        aside: String::new(),
        image: String::new(),
        fields: Vec::new(),
        closed: false,
        sign_in_required: true,
        anonymized: false,
        credits_participation: true,
        repetitions_field: None,
        next_url: String::new(),
        confirmation_message: String::new(),
        first_submission_email: None,
        project: None,
        owners: Vec::new(),
        map: None,
        created: now.clone(),
        updated: now,
    };

    page("New form", editor(form, true, false).await?, user).await
}

#[derive(Debug, Deserialize)]
struct EditQuery {
    #[serde(default)]
    saved: bool,
}

/// GET `/-admin/forms/:form_id`: the editor for the form.
#[handler]
async fn edit(
    Authenticated(user): Authenticated,
    Path(form_id): Path<Ulid>,
    Query(query): Query<EditQuery>,
) -> Result<Response, Error> {
    let Some(form) = super::load(FormLookup::Id(form_id)).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let title = form.name.clone();
    page(&title, editor(form, false, query.saved).await?, user).await
}

#[derive(Debug, Deserialize)]
struct NewField {
    new_kind: String,
}

/// GET `/-admin/forms/field?new_kind=<kind>`: a blank card for a new field
/// of the kind.
#[handler]
async fn new_field(Query(query): Query<NewField>) -> Result<Response, Error> {
    let Ok(kind) = query.new_kind.parse::<Kind>() else {
        return Ok(StatusCode::BAD_REQUEST.into_response());
    };

    let card = FieldCard {
        token: Ulid::new().to_string(),
        field: FormField {
            key: FieldKey::from(""),
            label: String::new(),
            help: String::new(),
            required: false,
            visible_if: None,
            remember: false,
            default: None,
            kind: kind.blank(),
        },
    };

    Ok(Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(card.render_once()?))
}

fn summary(form: &FormDefinition) -> FormSummary {
    FormSummary {
        form_id: form.form_id,
        orrery_id: form.orrery_id,
        slug: form.slug.clone(),
        name: form.name.clone(),
        closed: form.closed,
        project: form.project.clone(),
    }
}

const KEPT_BY_ORRERY: &str = "This form is a copy of one of orrery’s, which orrery keeps until the \
     switch to hydra. Change it on orrery.";

/// POST `/-admin/forms/:form_id`: keep the form as the editor sent it, and
/// give it its page; or show the editor again with why it was refused.
#[handler]
async fn save(
    Authenticated(user): Authenticated,
    Path(form_id): Path<Ulid>,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Result<Response, Error> {
    let kept = super::load(FormLookup::Id(form_id)).await?;
    let is_new = kept.is_none();

    let mut draft = read_draft(&pairs, form_id, kept.as_ref());
    resolve(&mut draft).await?;

    if draft.errors.is_empty() {
        let saved: FormSaved = request("user.forms.save", &draft.form).await?;

        let refusal = match saved {
            FormSaved::Saved => None,
            FormSaved::SlugTaken => Some(format!(
                "Another form has the address /form/{}.",
                draft.form.slug.as_deref().unwrap_or_default()
            )),
            FormSaved::MainTaken => Some(
                "The project already has a main form. Make this one another form it offers, \
                 or make the main one so first."
                    .to_string(),
            ),
            FormSaved::Inconsistent(why) => Some(why),
            FormSaved::KeptByOrrery => Some(KEPT_BY_ORRERY.to_string()),
        };

        match refusal {
            None => {
                super::entities::reconcile(&summary(&draft.form)).await?;

                if let Some(user) = &user {
                    log_activity(
                        None,
                        "form-save",
                        user.user_id,
                        form_id,
                        draft.form.name.clone(),
                    )
                    .await?;
                }

                return Ok(
                    Redirect::see_other(format!("/-admin/forms/{form_id}?saved=true"))
                        .into_response(),
                );
            }
            Some(refusal) => draft.errors.push(refusal),
        }
    }

    let title = if is_new {
        "New form".to_string()
    } else {
        draft.form.name.clone()
    };
    let panel = drafted(
        draft.form,
        is_new,
        draft.project,
        draft.role,
        draft.owners,
        draft.errors,
        false,
    )
    .await?;
    let mut response = page(&title, panel, user).await?;
    response.set_status(StatusCode::UNPROCESSABLE_ENTITY);

    Ok(response)
}

/// POST `/-admin/forms/:form_id/delete`: delete the form, every submission
/// of it, and its page.
#[handler]
async fn delete(
    Authenticated(user): Authenticated,
    Path(form_id): Path<Ulid>,
) -> Result<Response, Error> {
    let deleted: FormDeleted = request("user.forms.delete", form_id).await?;

    match deleted {
        FormDeleted::Deleted => {
            Entity::unpublish_page(EntityKind::Form, form_id).await?;

            if let Some(user) = &user {
                log_activity(None, "form-delete", user.user_id, form_id, None::<String>).await?;
            }
        }
        FormDeleted::Missing => {}
        FormDeleted::KeptByOrrery => {
            let Some(form) = super::load(FormLookup::Id(form_id)).await? else {
                return Ok(Redirect::see_other("/-admin/forms").into_response());
            };

            let title = form.name.clone();
            let mut panel = editor(form, false, false).await?;
            panel.errors.push(KEPT_BY_ORRERY.to_string());
            let mut response = page(&title, panel, user).await?;
            response.set_status(StatusCode::CONFLICT);

            return Ok(response);
        }
    }

    Ok(Redirect::see_other("/-admin/forms").into_response())
}

#[cfg(test)]
mod tests;
