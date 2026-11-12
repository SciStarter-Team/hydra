//! The copy of orrery's forms and their submissions kept until cutover.
//!
//! Forms come as one snapshot (`/staff/sync/forms`). A form built in
//! orrery's form builder becomes its fields here, each keyed by its question
//! as orrery keyed its answers. A form orrery drew with a page of its own (a
//! `component`) takes its fields from the definition seeded here for it, if
//! there is one; one there is none for is copied closed, since nothing here
//! can show it.
//!
//! Submissions come a page at a time, in the order orrery last changed
//! them (`/staff/sync/form_data`). Orrery stamps that change when it saves a
//! row, not when the save is committed, so a row can appear behind the
//! cursor; each round reads again from a few minutes behind where the last
//! ended, and keeping a row twice changes nothing. Deletions are found by
//! comparing every id orrery still has (`/staff/sync/form_data_ids`).
//!
//! Orrery's drafts, and the empty submissions its own pages made by mistake,
//! are not copied.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use anyhow::{Error, anyhow};
use common::{
    message::{
        form::{
            Answer, Answers, BooleanStyle, ChoiceOption, ChoiceWidget, FieldKey, FieldKind,
            FormDefinition, FormField, FormLookup, FormMap, FormProject, FormProjectRole,
            SubmissionRecord, Verdict, legacy_is_empty,
        },
        location::Point,
    },
    misc,
    shared::db,
    timestring::TimeString,
    transaction::Immediate,
    ulid::AsUlid,
};
use sqlx::SqlitePool;
use tokio::time::sleep;
use ulid::Ulid;

/// How far behind its cursor each round reads submissions again.
const OVERLAP: jiff::SignedDuration = jiff::SignedDuration::from_mins(5);

/// How often the ids orrery still has are compared.
const DELETION_CHECK: Duration = Duration::from_secs(60 * 60);

/// Orrery's page of submissions holds at most this many.
const PAGE: usize = 500;

const CURSOR_CHANGED: &str = "form-data-sync-changed";
const CURSOR_ID: &str = "form-data-sync-id";

/// The point orrery put a submission at when it knew nowhere better.
const ORRERY_DEFAULT_POINT: (f64, f64) = (-97.0, 38.0);

/// One of orrery's forms, as `/staff/sync/forms` sends it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OrreryForm {
    pub id: i64,
    pub slug: Option<String>,
    pub name: String,
    pub question: String,
    pub image: String,
    pub field_json: String,
    pub component: String,
    pub closed: bool,
    pub approved: bool,
    pub identity_not_required: bool,
    pub anonymized: bool,
    pub participation: bool,
    pub repetitions_field: String,
    pub next: String,
    pub map: bool,
    pub confirmation_message: String,
    pub first_submission_message: String,
    pub entity_ulid: Option<String>,
    pub worksheet_for_ulid: Option<String>,
    pub main_for: Vec<String>,
    /// Who the form belongs to (`core_user_forms`).
    #[serde(default)]
    pub owner_ulids: Vec<String>,
}

/// One of orrery's submissions, as `/staff/sync/form_data` sends it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OrrerySubmission {
    pub id: i64,
    pub form_id: i64,
    pub when: String,
    pub submitter_ulid: Option<String>,
    #[serde(rename = "final")]
    pub final_: bool,
    pub data_json: String,
    pub evaluation: String,
    pub comment: String,
    pub changed: String,
    pub lon: Option<f64>,
    pub lat: Option<f64>,
}

/// What a form orrery drew with a page of its own is here: the fields and
/// prose that page had, written out as a definition.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Seed {
    #[serde(default)]
    pub intro: String,
    #[serde(default)]
    pub aside: String,
    pub fields: Vec<FormField>,
    #[serde(default)]
    pub map: Option<FormMap>,
}

/// The seeded definitions, by orrery's id for the form.
pub(crate) const SEEDS: &[(i64, &str)] = &[
    (5, include_str!("seeds/5.json")),
    (6, include_str!("seeds/6.json")),
    (8, include_str!("seeds/8.json")),
    (9, include_str!("seeds/9.json")),
    (13, include_str!("seeds/13.json")),
];

fn seed(orrery_id: i64) -> Result<Option<Seed>, Error> {
    SEEDS
        .iter()
        .find(|(id, _)| *id == orrery_id)
        .map(|(_, json)| serde_json::from_str(json).map_err(Error::from))
        .transpose()
}

/// Copy orrery's forms and their submissions, until hydra keeps its own.
pub async fn synchronize() {
    if !common::primary::CopyKeeper::NOW.orrery_keeps_copies() {
        return;
    }

    let mut deletions_checked: Option<Instant> = None;

    loop {
        let pause = match round(&mut deletions_checked).await {
            Ok(()) => Duration::from_secs(15 * 60),
            Err(e) => {
                eprintln!("Form sync error: {e}");
                Duration::from_secs(60)
            }
        };

        sleep(pause).await;
    }
}

async fn round(deletions_checked: &mut Option<Instant>) -> Result<(), Error> {
    let client = reqwest::Client::new();

    let forms: Vec<OrreryForm> = client
        .get("https://scistarter.org/staff/sync/forms")
        .bearer_auth(env!("SYNC_TOKEN"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    mirror_forms(db(), &forms).await?;

    let mut copied = CopiedForms::new(&forms);
    let (changed, id) = cursor().await?;
    let mut since = changed
        .map(|changed| changed - OVERLAP)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    let mut last_id = if changed.is_some() { 0 } else { id };

    loop {
        let page: Vec<OrrerySubmission> = client
            .get("https://scistarter.org/staff/sync/form_data")
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

        for submission in &page {
            mirror_submission(db(), &mut copied, submission)
                .await
                .map_err(|e| {
                    anyhow!(
                        "submission {} of form {}: {e}",
                        submission.id,
                        submission.form_id
                    )
                })?;

            since = submission.changed.parse()?;
            last_id = submission.id;
            advance_cursor(since, last_id).await?;
        }

        if page.len() < PAGE {
            break;
        }
    }

    if deletions_checked.is_none_or(|checked| checked.elapsed() >= DELETION_CHECK) {
        let ids: Vec<i64> = client
            .get("https://scistarter.org/staff/sync/form_data_ids")
            .bearer_auth(env!("SYNC_TOKEN"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        drop_deleted_submissions(db(), &ids).await?;
        *deletions_checked = Some(Instant::now());
    }

    Ok(())
}

/// Where the last round stopped: the time orrery last changed the last
/// submission it copied, and that submission's id.
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

/// Make the forms here those of orrery's snapshot `forms`: each as orrery has
/// it, keeping the id it has here, and none orrery no longer has.
pub(crate) async fn mirror_forms(pool: &SqlitePool, forms: &[OrreryForm]) -> Result<(), Error> {
    // Orrery has had forms all along; a snapshot without any is a fault on
    // its side, and taken at its word would delete every form here and every
    // submission of them.
    if forms.is_empty() {
        return Err(anyhow!("orrery sent no forms"));
    }

    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;
    let now = TimeString::now();

    // Each project's main form, as each form is given its role: orrery can
    // name two forms the main one of a project, and the first is.
    let mut mains: BTreeSet<String> = BTreeSet::new();

    for form in forms {
        let kept = super::load(&mut tx, &FormLookup::Orrery(form.id)).await?;
        let mut definition = definition_of(form, kept.as_ref(), &now)?;
        let form_id = definition.form_id.to_string();

        // An owner not copied here yet is taken in by a later round, once
        // they are; left in, they would keep the form from ever matching
        // its copy.
        definition.owners = super::known_users(&mut tx, &definition.owners).await?;

        // A slug or a main form can pass from one of orrery's forms to
        // another between snapshots; the one losing it gives it up here,
        // and takes what it has now when its own turn comes. A form made
        // here keeps what it has.
        if let Some(slug) = &definition.slug {
            sqlx::query!(
                r#"UPDATE "forms" SET "slug" = NULL
                   WHERE "slug" = ? AND "form_id" <> ? AND "orrery_id" IS NOT NULL"#,
                slug,
                form_id
            )
            .execute(&mut tx)
            .await?;

            if super::slug_taken(&mut tx, &form_id, slug).await? {
                eprintln!(
                    "Orrery's form {} has the slug {slug:?}, which a form made here has",
                    form.id
                );
                definition.slug = None;
            }
        }

        if let Some(project) = &mut definition.project
            && project.role == FormProjectRole::Main
        {
            if mains.insert(project.project_id.clone()) {
                sqlx::query!(
                    r#"UPDATE "forms" SET "project_role" = 'extra'
                       WHERE "project_id" = ? AND "project_role" = 'main'
                         AND "form_id" <> ? AND "orrery_id" IS NOT NULL"#,
                    project.project_id,
                    form_id
                )
                .execute(&mut tx)
                .await?;
            }

            if super::main_taken(&mut tx, &form_id, &project.project_id).await? {
                project.role = FormProjectRole::Extra;
            }
        }

        let unchanged = kept.as_ref().is_some_and(|kept| {
            FormDefinition {
                updated: kept.updated.clone(),
                ..definition.clone()
            } == *kept
        });

        if !unchanged {
            super::store(&mut tx, &definition).await?;
        }
    }

    let ids = serde_json::to_string(&forms.iter().map(|form| form.id).collect::<Vec<_>>())?;
    sqlx::query!(
        r#"DELETE FROM "forms"
           WHERE "orrery_id" IS NOT NULL
             AND "orrery_id" NOT IN (SELECT "value" FROM json_each(?))"#,
        ids
    )
    .execute(&mut tx)
    .await?;

    tx.commit().await?;

    Ok(())
}

/// What orrery's form `form` is here, where `kept` is the copy already here.
fn definition_of(
    form: &OrreryForm,
    kept: Option<&FormDefinition>,
    now: &TimeString,
) -> Result<FormDefinition, Error> {
    let project = [
        form.entity_ulid.as_ref(),
        form.main_for.first(),
        form.worksheet_for_ulid.as_ref(),
    ]
    .into_iter()
    .flatten()
    .next()
    .map(|project_id| FormProject {
        project_id: project_id.clone(),
        role: if form.main_for.contains(project_id) {
            FormProjectRole::Main
        } else {
            FormProjectRole::Extra
        },
    });

    let (intro, aside, fields, map, drawable) = if form.component.is_empty() {
        (
            form.question.clone(),
            String::new(),
            fields_of(&form.field_json),
            form.map.then(|| FormMap {
                intro: String::new(),
                popup_fields: Vec::new(),
            }),
            true,
        )
    } else {
        match seed(form.id)? {
            Some(seed) => (seed.intro, seed.aside, seed.fields, seed.map, true),
            None => (
                form.question.clone(),
                String::new(),
                Vec::new(),
                None,
                false,
            ),
        }
    };

    let mut definition = FormDefinition {
        form_id: kept.map_or_else(Ulid::new, |kept| kept.form_id),
        orrery_id: Some(form.id),
        slug: form.slug.clone().filter(|slug| !slug.is_empty()),
        name: form.name.clone(),
        intro,
        aside,
        image: form.image.clone(),
        fields,
        closed: form.closed || !form.approved || !drawable,
        sign_in_required: !form.identity_not_required,
        anonymized: form.anonymized,
        credits_participation: form.participation,
        repetitions_field: Some(form.repetitions_field.as_str())
            .filter(|key| !key.is_empty())
            .map(FieldKey::from),
        next_url: form.next.clone(),
        confirmation_message: form.confirmation_message.clone(),
        first_submission_email: Some(form.first_submission_message.clone())
            .filter(|slug| !slug.is_empty()),
        project,
        owners: form
            .owner_ulids
            .iter()
            .map(|ulid| ulid.as_ulid())
            .collect::<Result<_, _>>()?,
        map,
        created: kept.map_or_else(|| now.clone(), |kept| kept.created.clone()),
        updated: now.clone(),
    };

    // Orrery's form builder checked none of what a definition here is held
    // to, so drop what does not fit rather than refuse the form.
    if definition.repetitions_field.as_ref().is_some_and(|key| {
        !matches!(
            definition.field(key.as_str()).map(|field| &field.kind),
            Some(FieldKind::Number { .. })
        )
    }) {
        definition.repetitions_field = None;
    }

    Ok(definition)
}

/// The fields of a form built in orrery's form builder, from its
/// `field_json`: a list of `{type, prompt, extra, required}`. Each question
/// is keyed by its prompt, as orrery keyed its answers; a prompt asked twice
/// is told apart by a number, as only the first could have had its answer
/// kept under it. A field of no type orrery drew is left out, as orrery
/// showed nothing for it.
pub(crate) fn fields_of(field_json: &str) -> Vec<FormField> {
    #[derive(serde::Deserialize)]
    struct OrreryField {
        #[serde(rename = "type", default)]
        kind: String,
        #[serde(default)]
        prompt: String,
        #[serde(default)]
        extra: serde_json::Value,
        #[serde(default)]
        required: bool,
    }

    let Ok(listed) = serde_json::from_str::<Vec<OrreryField>>(field_json) else {
        return Vec::new();
    };

    let mut fields: Vec<FormField> = Vec::new();

    for (i, orrery) in listed.into_iter().enumerate() {
        let extra = match &orrery.extra {
            serde_json::Value::String(text) => text.clone(),
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        };

        let options = || {
            let mut options: Vec<ChoiceOption> = Vec::new();

            for option in extra.split(',').map(str::trim).filter(|o| !o.is_empty()) {
                if !options.iter().any(|kept| kept.value == option) {
                    options.push(ChoiceOption {
                        value: option.to_string(),
                        label: option.to_string(),
                    });
                }
            }

            options
        };

        let (kind, label) = match orrery.kind.as_str() {
            "heading" => (FieldKind::Heading, orrery.prompt.clone()),
            "static-image" => (FieldKind::Image { url: extra.clone() }, String::new()),
            "static-text" => (
                FieldKind::Markdown {
                    text: extra.clone(),
                },
                String::new(),
            ),
            "short-text" | "long-text" => (
                FieldKind::Text {
                    multiline: orrery.kind == "long-text",
                    placeholder: String::new(),
                },
                orrery.prompt.clone(),
            ),
            "number" => (
                FieldKind::Number {
                    min: None,
                    max: None,
                    integer: false,
                },
                orrery.prompt.clone(),
            ),
            "select-one" | "select-many" => (
                FieldKind::Choice {
                    options: options(),
                    multiple: orrery.kind == "select-many",
                    widget: ChoiceWidget::Select,
                },
                orrery.prompt.clone(),
            ),
            "rating" => (
                FieldKind::Choice {
                    options: options(),
                    multiple: false,
                    widget: ChoiceWidget::Buttons,
                },
                orrery.prompt.clone(),
            ),
            "yes-no" | "true-false" | "checkbox" => (
                FieldKind::Boolean {
                    style: match orrery.kind.as_str() {
                        "yes-no" => BooleanStyle::YesNo,
                        "true-false" => BooleanStyle::TrueFalse,
                        _ => BooleanStyle::Checkbox,
                    },
                },
                orrery.prompt.clone(),
            ),
            "date" => (
                FieldKind::Date {
                    not_in_future: false,
                },
                orrery.prompt.clone(),
            ),
            "time" => (
                FieldKind::Time {
                    min: None,
                    max: None,
                    step_minutes: None,
                    not_before: None,
                },
                orrery.prompt.clone(),
            ),
            "datetime" => (FieldKind::DateTime, orrery.prompt.clone()),
            "picture" => (FieldKind::Photo { multiple: false }, orrery.prompt.clone()),
            "location" => (FieldKind::Location, orrery.prompt.clone()),
            _ => continue,
        };

        let key = if kind.asks() {
            let mut key = orrery.prompt.clone();
            let mut n = 1;
            while fields.iter().any(|field| field.key.as_str() == key) {
                n += 1;
                key = format!("{} ({n})", orrery.prompt);
            }
            key
        } else {
            format!("{}-{i}", orrery.kind)
        };

        fields.push(FormField {
            key: FieldKey(key),
            label,
            help: String::new(),
            required: orrery.required && kind.asks(),
            visible_if: None,
            remember: false,
            // Orrery's number fields started at 0.
            default: matches!(kind, FieldKind::Number { .. }).then(|| "0".to_string()),
            kind,
        });
    }

    fields
}

/// The forms here that orrery's submissions belong to, read once a round:
/// each by orrery's id for it, with whether it asked for a place.
pub(crate) struct CopiedForms {
    /// Orrery's ids for the forms it drew with a page of its own.
    drawn_by_own_page: BTreeSet<i64>,
    forms: HashMap<i64, Option<(FormDefinition, bool)>>,
}

impl CopiedForms {
    pub(crate) fn new(forms: &[OrreryForm]) -> CopiedForms {
        CopiedForms {
            drawn_by_own_page: forms
                .iter()
                .filter(|form| !form.component.is_empty())
                .map(|form| form.id)
                .collect(),
            forms: HashMap::new(),
        }
    }

    async fn get(
        &mut self,
        pool: &SqlitePool,
        orrery_id: i64,
    ) -> Result<Option<&(FormDefinition, bool)>, Error> {
        if !self.forms.contains_key(&orrery_id) {
            let form = super::load(pool, &FormLookup::Orrery(orrery_id)).await?;
            let copied = form.map(|form| {
                // Every page orrery drew a form with had a map to pick a
                // place on.
                let asked = self.drawn_by_own_page.contains(&orrery_id)
                    || form
                        .fields
                        .iter()
                        .any(|field| field.kind == FieldKind::Location);
                (form, asked)
            });
            self.forms.insert(orrery_id, copied);
        }

        Ok(self.forms[&orrery_id].as_ref())
    }
}

/// Copy one of orrery's submissions, or drop the copy of one that is no
/// longer to be copied.
pub(crate) async fn mirror_submission(
    pool: &SqlitePool,
    copied: &mut CopiedForms,
    submission: &OrrerySubmission,
) -> Result<(), Error> {
    let Some((form, asked_for_place)) = copied.get(pool, submission.form_id).await? else {
        return Err(anyhow!("form {} has not been copied", submission.form_id));
    };

    let data: serde_json::Value = serde_json::from_str(&submission.data_json)?;

    if !submission.final_ || legacy_is_empty(&data) || all_empty(&data) {
        sqlx::query!(
            r#"DELETE FROM "form_submissions" WHERE "orrery_id" = ?"#,
            submission.id
        )
        .execute(pool)
        .await?;
        return Ok(());
    }

    let answers = answers_of(form, &data);

    let orrery_point = match (submission.lon, submission.lat) {
        (Some(longitude), Some(latitude)) if (longitude, latitude) != ORRERY_DEFAULT_POINT => {
            Some(Point {
                latitude,
                longitude,
            })
        }
        _ => None,
    };

    // A form that asked for a place keeps the place picked; orrery kept it
    // as its point too, which is the place where the form's answers lack
    // one. A form that did not ask put its point where the submitter's
    // address was looked up to be.
    let (location, ip_location) = if *asked_for_place {
        (form.location(&answers).or(orrery_point), None)
    } else {
        (None, orrery_point)
    };

    let kept_id = sqlx::query_scalar!(
        r#"SELECT "submission_id" AS "submission_id!" FROM "form_submissions"
           WHERE "orrery_id" = ?"#,
        submission.id
    )
    .fetch_optional(pool)
    .await?;

    let record = SubmissionRecord {
        submission_id: kept_id
            .map(|id| id.as_ulid())
            .transpose()?
            .unwrap_or_else(Ulid::new),
        orrery_id: Some(submission.id),
        submitter: submission
            .submitter_ulid
            .as_deref()
            .map(|ulid| ulid.as_ulid())
            .transpose()?,
        when: TimeString::try_from(submission.when.clone())?,
        answers,
        location,
        ip_address: None,
        ip_location,
        evaluation: verdict_of(submission),
        comment: submission.comment.clone(),
    };

    super::submissions::store(pool, form.form_id, &record).await
}

/// What orrery's submission was judged to be: nothing, where it was not
/// judged, or judged something not offered.
fn verdict_of(submission: &OrrerySubmission) -> Option<Verdict> {
    if submission.evaluation.is_empty() {
        return None;
    }

    submission
        .evaluation
        .parse()
        .inspect_err(|e| eprintln!("Orrery's submission {}: {e}", submission.id))
        .ok()
}

/// Whether every answer in orrery's `data_json` is one of the ways it stored
/// no answer: what its own pages saved by mistake on a second submission.
fn all_empty(data: &serde_json::Value) -> bool {
    match data {
        serde_json::Value::Object(answers) => answers.values().all(legacy_is_empty),
        _ => false,
    }
}

/// The answers in orrery's `data_json`, read as the form's fields' kinds,
/// with those under a key no field has kept as they were.
pub(crate) fn answers_of(form: &FormDefinition, data: &serde_json::Value) -> Answers {
    let mut answers = Answers::new();

    let serde_json::Value::Object(given) = data else {
        answers.insert(FieldKey::from("data_json"), Answer::Legacy(data.clone()));
        return answers;
    };

    for (key, value) in given {
        let answer = match form.field(key).filter(|field| field.kind.asks()) {
            Some(field) => field.legacy_answer(value),
            None => (!legacy_is_empty(value)).then(|| Answer::Legacy(value.clone())),
        };

        if let Some(answer) = answer {
            answers.insert(FieldKey(key.clone()), answer);
        }
    }

    answers
}

/// Delete the copies of the submissions orrery no longer has: those whose
/// ids are not among `ids`, every id it has.
pub(crate) async fn drop_deleted_submissions(pool: &SqlitePool, ids: &[i64]) -> Result<(), Error> {
    if ids.is_empty() {
        return Err(anyhow!("orrery sent no submission ids"));
    }

    let ids = serde_json::to_string(ids)?;
    sqlx::query!(
        r#"DELETE FROM "form_submissions"
           WHERE "orrery_id" IS NOT NULL
             AND "orrery_id" NOT IN (SELECT "value" FROM json_each(?))"#,
        ids
    )
    .execute(pool)
    .await?;

    Ok(())
}
