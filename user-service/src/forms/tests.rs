use std::collections::BTreeMap;

use assert2::check;
use common::message::{
    form::{
        Answer, ChoiceOption, ChoiceWidget, Evaluation, FieldKey, FieldKind, FormDefinition,
        FormField, FormLookup, FormMap, FormProject, FormProjectRole, FormSaved, FormSubmission,
        RememberedRequest, SubmissionOf, SubmissionsRequest, SubmitOutcome, Verdict,
    },
    location::Point,
};
use sqlx::SqlitePool;
use ulid::Ulid;

use super::mirror::{CopiedForms, OrreryForm, OrrerySubmission, Seed};
use super::*;

const ADA: &str = "01JADA0000000000000000000A";
const SQUIRREL: &str = "01JSQRL0000000000000000000";

/// The forms tables as their migration makes them, in a database of their
/// own, beside the accounts they refer to.
async fn fixture() -> SqlitePool {
    // SAFETY: mod_spatialite is the trusted extension every service loads.
    let options = unsafe {
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(":memory:")
            .foreign_keys(true)
            .extension("mod_spatialite")
    };

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("pool");

    sqlx::raw_sql(
        r#"SELECT InitSpatialMetadata(1);
           CREATE TABLE "users" ("user_id" TEXT PRIMARY KEY NOT NULL);
           CREATE TABLE "misc" ("key" TEXT PRIMARY KEY NOT NULL, "value");"#,
    )
    .execute(&pool)
    .await
    .expect("accounts");

    sqlx::raw_sql(include_str!(
        "../../migrations/20261111141801_hosted_forms.up.sql"
    ))
    .execute(&pool)
    .await
    .expect("schema");

    sqlx::query(r#"INSERT INTO "users" ("user_id") VALUES (?)"#)
        .bind(ADA)
        .execute(&pool)
        .await
        .expect("user");

    pool
}

fn ada() -> Ulid {
    ADA.parse().unwrap()
}

fn field(key: &str, kind: FieldKind) -> FormField {
    FormField {
        key: key.into(),
        label: key.to_string(),
        help: String::new(),
        required: false,
        visible_if: None,
        remember: false,
        default: None,
        kind,
    }
}

fn text() -> FieldKind {
    FieldKind::Text {
        multiline: false,
        placeholder: String::new(),
    }
}

fn definition(fields: Vec<FormField>) -> FormDefinition {
    FormDefinition {
        form_id: Ulid::new(),
        orrery_id: None,
        slug: None,
        name: "Squirrel count".to_string(),
        intro: String::new(),
        aside: String::new(),
        image: String::new(),
        fields,
        closed: false,
        sign_in_required: false,
        anonymized: false,
        credits_participation: false,
        repetitions_field: None,
        next_url: String::new(),
        confirmation_message: String::new(),
        first_submission_email: None,
        project: None,
        owners: Vec::new(),
        map: None,
        created: TimeString::now(),
        updated: TimeString::now(),
    }
}

/// A form asking how many squirrels were seen, where, and by what name to
/// remember the place.
fn squirrel_count() -> FormDefinition {
    definition(vec![
        FormField {
            required: true,
            ..field(
                "count",
                FieldKind::Number {
                    min: Some(0.0),
                    max: None,
                    integer: true,
                },
            )
        },
        field("where", FieldKind::Location),
        FormField {
            remember: true,
            ..field("site", text())
        },
    ])
}

fn raw(pairs: &[(&str, &str)]) -> common::message::form::RawAnswers {
    let mut raw: common::message::form::RawAnswers = BTreeMap::new();
    for (key, value) in pairs {
        raw.entry(key.to_string())
            .or_default()
            .push(value.to_string());
    }
    raw
}

fn submission(form: &FormDefinition, pairs: &[(&str, &str)]) -> FormSubmission {
    FormSubmission {
        form_id: form.form_id,
        submitter: Some(ada()),
        answers: raw(pairs),
        ip_address: None,
    }
}

async fn saved(pool: &SqlitePool, form: FormDefinition) -> FormDefinition {
    let form_id = form.form_id;
    check!(
        save_definition(pool, form, CopyKeeper::Hydra)
            .await
            .unwrap()
            == FormSaved::Saved
    );
    load(pool, &FormLookup::Id(form_id)).await.unwrap().unwrap()
}

async fn submitted(pool: &SqlitePool, submission: FormSubmission) -> SubmitOutcome {
    submissions::submit_form(pool, submission).await.unwrap()
}

async fn all_submissions(
    pool: &SqlitePool,
    form_id: Ulid,
) -> Vec<common::message::form::SubmissionRecord> {
    submissions::page(
        pool,
        &SubmissionsRequest {
            form_id,
            submitter: None,
            after: None,
            offset: 0,
            limit: 100,
        },
    )
    .await
    .unwrap()
    .submissions
}

/// Every submission of `form_id`, read `limit` at a time, each page after
/// the last of the one before; `between` is called between pages.
async fn paged_submissions(
    pool: &SqlitePool,
    form_id: Ulid,
    limit: u32,
    mut between: impl AsyncFnMut(),
) -> Vec<Ulid> {
    let mut read = Vec::new();

    loop {
        let page = submissions::page(
            pool,
            &SubmissionsRequest {
                form_id,
                submitter: None,
                after: read.last().copied(),
                offset: 0,
                limit,
            },
        )
        .await
        .unwrap();

        if page.submissions.is_empty() {
            return read;
        }

        read.extend(page.submissions.iter().map(|record| record.submission_id));
        between().await;
    }
}

#[tokio::test]
async fn reading_page_after_page_reads_each_submission_once() {
    let pool = fixture().await;
    let form = saved(&pool, squirrel_count()).await;

    for count in 0..5 {
        let count = count.to_string();
        submitted(&pool, submission(&form, &[("count", &count)])).await;
    }
    let before: Vec<Ulid> = all_submissions(&pool, form.form_id)
        .await
        .iter()
        .map(|record| record.submission_id)
        .collect();

    // Submissions arriving meanwhile are newer than every page still to
    // come, so they neither push one already read onto a later page nor
    // come up themselves.
    let read = paged_submissions(&pool, form.form_id, 2, async || {
        submitted(&pool, submission(&form, &[("count", "9")])).await;
    })
    .await;

    check!(read == before);
}

#[tokio::test]
async fn a_page_holds_no_more_than_fits_in_a_message() {
    let pool = fixture().await;
    let form = saved(&pool, squirrel_count()).await;
    let site = "x".repeat(300_000);

    for _ in 0..3 {
        submitted(&pool, submission(&form, &[("count", "1"), ("site", &site)])).await;
    }

    let mut pages = 0;
    let read = paged_submissions(&pool, form.form_id, 100, async || pages += 1).await;

    check!(read.len() == 3);
    check!(pages == 3);
}

#[tokio::test]
async fn a_saved_form_reads_back_by_each_of_its_names() {
    let pool = fixture().await;
    let mut form = squirrel_count();
    form.slug = Some("squirrels".to_string());
    form.orrery_id = Some(5);
    form.map = Some(FormMap {
        intro: "Where they were".to_string(),
        popup_fields: vec!["count".into()],
    });
    form.project = Some(FormProject {
        project_id: SQUIRREL.to_string(),
        role: FormProjectRole::Main,
    });

    let kept = saved(&pool, form.clone()).await;
    check!(kept.fields == form.fields);
    check!(kept.map == form.map);
    check!(kept.project == form.project);

    for lookup in [
        FormLookup::Id(form.form_id),
        FormLookup::Orrery(5),
        FormLookup::Slug("squirrels".to_string()),
    ] {
        check!(load(&pool, &lookup).await.unwrap().as_ref() == Some(&kept));
    }

    let listed = summaries(&pool, Listed::OfProject(SQUIRREL)).await.unwrap();
    check!(listed.len() == 1);
    check!(listed[0].slug.as_deref() == Some("squirrels"));
}

#[tokio::test]
async fn a_form_is_owned_only_by_people_who_are_here() {
    let pool = fixture().await;
    let stranger = Ulid::new();
    let mut form = squirrel_count();
    form.owners = vec![ada(), stranger];

    let kept = saved(&pool, form.clone()).await;
    check!(kept.owners == [ada()]);

    let owned = summaries(&pool, Listed::OwnedBy(ada())).await.unwrap();
    check!(owned.iter().map(|form| form.form_id).collect::<Vec<_>>() == [form.form_id]);
    check!(summaries(&pool, Listed::OwnedBy(stranger)).await.unwrap() == []);

    form.owners = Vec::new();
    check!(saved(&pool, form).await.owners == []);
    check!(summaries(&pool, Listed::OwnedBy(ada())).await.unwrap() == []);
}

#[tokio::test]
async fn saving_keeps_when_a_form_was_made() {
    let pool = fixture().await;
    let form = saved(&pool, squirrel_count()).await;

    let mut changed = form.clone();
    changed.name = "Squirrels seen".to_string();
    changed.created = TimeString::now();
    let changed = saved(&pool, changed).await;

    check!(changed.created == form.created);
    check!(changed.name == "Squirrels seen");
}

#[tokio::test]
async fn a_slug_or_a_main_form_is_one_forms_alone() {
    let pool = fixture().await;
    let project = Some(FormProject {
        project_id: SQUIRREL.to_string(),
        role: FormProjectRole::Main,
    });

    let mut first = squirrel_count();
    first.slug = Some("squirrels".to_string());
    first.project = project.clone();
    saved(&pool, first).await;

    let mut same_slug = squirrel_count();
    same_slug.slug = Some("squirrels".to_string());
    check!(
        save_definition(&pool, same_slug, CopyKeeper::Hydra)
            .await
            .unwrap()
            == FormSaved::SlugTaken
    );

    let mut second_main = squirrel_count();
    second_main.project = project.clone();
    check!(
        save_definition(&pool, second_main.clone(), CopyKeeper::Hydra)
            .await
            .unwrap()
            == FormSaved::MainTaken
    );

    second_main.project = Some(FormProject {
        project_id: SQUIRREL.to_string(),
        role: FormProjectRole::Extra,
    });
    check!(
        save_definition(&pool, second_main, CopyKeeper::Hydra)
            .await
            .unwrap()
            == FormSaved::Saved
    );

    let twice = definition(vec![field("a", text()), field("a", text())]);
    check!(matches!(
        save_definition(&pool, twice, CopyKeeper::Hydra)
            .await
            .unwrap(),
        FormSaved::Inconsistent(_)
    ));
}

#[tokio::test]
async fn a_copy_orrery_keeps_is_changed_and_deleted_only_after_cutover() {
    let pool = fixture().await;

    let mut copy = squirrel_count();
    copy.orrery_id = Some(5);
    let copy = saved(&pool, copy).await;
    let made_here = saved(&pool, squirrel_count()).await;

    let mut renamed = copy.clone();
    renamed.name = "Renamed".to_string();
    check!(
        save_definition(&pool, renamed.clone(), CopyKeeper::Orrery)
            .await
            .unwrap()
            == FormSaved::KeptByOrrery
    );

    // Nor can a form be made a copy, or a copy be replaced by a form that
    // claims to be none.
    let mut claimed = made_here.clone();
    claimed.orrery_id = Some(6);
    check!(
        save_definition(&pool, claimed, CopyKeeper::Orrery)
            .await
            .unwrap()
            == FormSaved::KeptByOrrery
    );
    let mut disowned = copy.clone();
    disowned.orrery_id = None;
    check!(
        save_definition(&pool, disowned, CopyKeeper::Orrery)
            .await
            .unwrap()
            == FormSaved::KeptByOrrery
    );

    check!(
        delete_form(&pool, copy.form_id, CopyKeeper::Orrery)
            .await
            .unwrap()
            == FormDeleted::KeptByOrrery
    );
    check!(load(&pool, &FormLookup::Id(copy.form_id)).await.unwrap() == Some(copy.clone()));

    check!(
        delete_form(&pool, made_here.form_id, CopyKeeper::Orrery)
            .await
            .unwrap()
            == FormDeleted::Deleted
    );
    check!(
        delete_form(&pool, made_here.form_id, CopyKeeper::Orrery)
            .await
            .unwrap()
            == FormDeleted::Missing
    );

    check!(
        save_definition(&pool, renamed, CopyKeeper::Hydra)
            .await
            .unwrap()
            == FormSaved::Saved
    );
    check!(
        delete_form(&pool, copy.form_id, CopyKeeper::Hydra)
            .await
            .unwrap()
            == FormDeleted::Deleted
    );
}

#[tokio::test]
async fn a_submission_is_held_to_its_form() {
    let pool = fixture().await;
    let form = saved(&pool, squirrel_count()).await;

    let refused = submitted(&pool, submission(&form, &[("count", "")])).await;
    check!(
        matches!(refused, SubmitOutcome::Refused(errors) if errors[0].key == FieldKey::from("count"))
    );

    let mut closed = form.clone();
    closed.closed = true;
    let closed = saved(&pool, closed).await;
    check!(submitted(&pool, submission(&closed, &[("count", "3")])).await == SubmitOutcome::Closed);

    let mut members_only = closed.clone();
    members_only.closed = false;
    members_only.sign_in_required = true;
    let members_only = saved(&pool, members_only).await;
    let anonymous = FormSubmission {
        submitter: None,
        ..submission(&members_only, &[("count", "3")])
    };
    check!(submitted(&pool, anonymous).await == SubmitOutcome::SignInRequired);

    let elsewhere = FormSubmission {
        form_id: Ulid::new(),
        ..submission(&form, &[("count", "3")])
    };
    check!(submitted(&pool, elsewhere).await == SubmitOutcome::NoSuchForm);

    check!(all_submissions(&pool, form.form_id).await.is_empty());
}

#[tokio::test]
async fn a_kept_submission_reads_back_with_the_place_picked() {
    let pool = fixture().await;
    let form = saved(&pool, squirrel_count()).await;

    let outcome = submitted(
        &pool,
        submission(
            &form,
            &[("count", "3"), ("where", "38.5,-121.75"), ("site", "Oak")],
        ),
    )
    .await;
    let SubmitOutcome::Saved { submission_id } = outcome else {
        panic!("refused: {outcome:?}");
    };

    let kept = all_submissions(&pool, form.form_id).await;
    check!(kept.len() == 1);
    check!(kept[0].submission_id == submission_id);
    check!(kept[0].submitter == Some(ada()));
    check!(kept[0].answers[&FieldKey::from("count")] == Answer::Number(3.0));
    check!(
        kept[0].location
            == Some(Point {
                latitude: 38.5,
                longitude: -121.75
            })
    );
    check!(kept[0].ip_location == None);
}

#[tokio::test]
async fn only_the_answers_a_form_remembers_are_offered_again() {
    let pool = fixture().await;
    let form = saved(&pool, squirrel_count()).await;

    for (count, site) in [("3", "Oak"), ("4", "Elm")] {
        submitted(
            &pool,
            submission(&form, &[("count", count), ("site", site)]),
        )
        .await;
    }

    let remembered = submissions::remembered_answers(
        &pool,
        &RememberedRequest {
            form_id: form.form_id,
            user_id: ada(),
        },
    )
    .await
    .unwrap();

    check!(remembered.len() == 1);
    check!(remembered[&FieldKey::from("site")] == Answer::Text("Elm".to_string()));
}

#[tokio::test]
async fn a_map_shows_only_its_fields_and_blurs_an_anonymized_form() {
    let pool = fixture().await;
    let mut form = squirrel_count();
    form.anonymized = true;
    form.map = Some(FormMap {
        intro: String::new(),
        popup_fields: vec!["count".into()],
    });
    let form = saved(&pool, form).await;

    submitted(
        &pool,
        submission(
            &form,
            &[
                ("count", "3"),
                ("where", "38.54321,-121.75432"),
                ("site", "Oak"),
            ],
        ),
    )
    .await;
    submitted(&pool, submission(&form, &[("count", "1")])).await;

    let markers = submissions::markers(&pool, form.form_id).await.unwrap();
    check!(markers.len() == 1);
    check!(
        markers[0].location
            == Point {
                latitude: 38.54,
                longitude: -121.75
            }
    );
    check!(markers[0].answers.keys().collect::<Vec<_>>() == vec![&FieldKey::from("count")]);
}

#[tokio::test]
async fn a_map_holds_no_more_markers_than_fit_in_a_message() {
    let pool = fixture().await;
    let mut form = squirrel_count();
    form.map = Some(FormMap {
        intro: String::new(),
        popup_fields: vec!["site".into()],
    });
    let form = saved(&pool, form).await;
    let site = "x".repeat(250_000);

    for _ in 0..3 {
        submitted(
            &pool,
            submission(
                &form,
                &[("count", "1"), ("where", "38.5,-121.7"), ("site", &site)],
            ),
        )
        .await;
    }

    check!(
        submissions::markers(&pool, form.form_id)
            .await
            .unwrap()
            .len()
            == 2
    );
}

#[tokio::test]
async fn a_submission_is_judged_and_deleted_only_through_its_own_form() {
    let pool = fixture().await;
    let form = saved(&pool, squirrel_count()).await;
    let other = saved(&pool, squirrel_count()).await;
    submitted(&pool, submission(&form, &[("count", "1")])).await;
    let submission_id = all_submissions(&pool, form.form_id).await[0].submission_id;

    let through = |form_id| SubmissionOf {
        form_id,
        submission_id,
    };
    let judgement = |form_id| Evaluation {
        submission: through(form_id),
        evaluation: Some(Verdict::Negative),
        comment: "A honey bee, not a zombie".to_string(),
    };

    check!(
        !submissions::judge(&pool, &judgement(other.form_id))
            .await
            .unwrap()
    );
    check!(all_submissions(&pool, form.form_id).await[0].evaluation == None);
    check!(
        submissions::judge(&pool, &judgement(form.form_id))
            .await
            .unwrap()
    );
    check!(all_submissions(&pool, form.form_id).await[0].evaluation == Some(Verdict::Negative));

    check!(
        !submissions::remove(&pool, through(other.form_id))
            .await
            .unwrap()
    );
    check!(
        submissions::remove(&pool, through(form.form_id))
            .await
            .unwrap()
    );
    check!(all_submissions(&pool, form.form_id).await == []);
}

#[test]
fn orrerys_form_builder_fields_become_fields_keyed_by_their_prompts() {
    let field_json = r#"[
        {"type": "heading", "prompt": "About you", "extra": "", "id": 1},
        {"type": "short-text", "prompt": "Name", "extra": "", "id": 2, "required": true},
        {"type": "select-one", "prompt": "Habitat", "extra": "Wood, Field ,Wood,", "id": 3},
        {"type": "rating", "prompt": "Fun", "extra": "1,2,3", "id": 4},
        {"type": "yes-no", "prompt": "Again?", "extra": "", "id": 5},
        {"type": "", "prompt": "Nothing", "extra": "", "id": 6},
        {"type": "short-text", "prompt": "Name", "extra": "", "id": 7},
        {"type": "static-text", "prompt": "", "extra": "**Thanks**", "id": 8}
    ]"#;

    let fields = mirror::fields_of(field_json);
    let keys: Vec<&str> = fields.iter().map(|field| field.key.as_str()).collect();
    check!(
        keys == [
            "heading-0",
            "Name",
            "Habitat",
            "Fun",
            "Again?",
            "Name (2)",
            "static-text-7"
        ]
    );

    check!(fields[1].required);
    check!(
        fields[2].kind
            == FieldKind::Choice {
                options: ["Wood", "Field"]
                    .into_iter()
                    .map(|option| ChoiceOption {
                        value: option.to_string(),
                        label: option.to_string(),
                    })
                    .collect(),
                multiple: false,
                widget: ChoiceWidget::Select,
            }
    );
    check!(matches!(
        &fields[3].kind,
        FieldKind::Choice {
            widget: ChoiceWidget::Buttons,
            ..
        }
    ));
    check!(
        fields[6].kind
            == FieldKind::Markdown {
                text: "**Thanks**".to_string()
            }
    );

    check!(mirror::fields_of("{}").is_empty());
}

fn orrery_form(id: i64, field_json: &str) -> OrreryForm {
    OrreryForm {
        id,
        slug: None,
        name: format!("Form {id}"),
        question: "Count them".to_string(),
        image: String::new(),
        field_json: field_json.to_string(),
        component: String::new(),
        closed: false,
        approved: true,
        identity_not_required: false,
        anonymized: false,
        participation: true,
        repetitions_field: String::new(),
        next: String::new(),
        map: false,
        confirmation_message: String::new(),
        first_submission_message: String::new(),
        entity_ulid: None,
        worksheet_for_ulid: None,
        main_for: Vec::new(),
        owner_ulids: Vec::new(),
    }
}

#[tokio::test]
async fn orrerys_forms_are_copied_and_follow_its_changes() {
    let pool = fixture().await;

    let mut counted = orrery_form(
        137,
        r#"[{"type": "number", "prompt": "Bees", "extra": ""}]"#,
    );
    counted.slug = Some("bees".to_string());
    counted.main_for = vec![SQUIRREL.to_string()];
    counted.repetitions_field = "Bees".to_string();
    counted.first_submission_message = "first-contribution-GSPform".to_string();
    // One owner is a user here; the other is not yet, and keeps the copy
    // from changing on every round until they are.
    counted.owner_ulids = vec![ADA.to_string(), Ulid::new().to_string()];

    let mut second = orrery_form(138, "[]");
    second.main_for = vec![SQUIRREL.to_string()];
    second.approved = false;

    let mut drawn = orrery_form(17, "[]");
    drawn.component = "ss-crowd-the-tap".to_string();

    let mut seeded = orrery_form(5, "[]");
    seeded.component = "ss-project-squirrel".to_string();

    mirror::mirror_forms(
        &pool,
        &[
            counted.clone(),
            second.clone(),
            drawn.clone(),
            seeded.clone(),
        ],
    )
    .await
    .unwrap();

    let first_copy = load(&pool, &FormLookup::Orrery(137))
        .await
        .unwrap()
        .unwrap();
    check!(first_copy.owners == [ada()]);
    check!(first_copy.slug.as_deref() == Some("bees"));
    check!(first_copy.repetitions_field == Some("Bees".into()));
    check!(first_copy.first_submission_email.as_deref() == Some("first-contribution-GSPform"));
    check!(first_copy.project.as_ref().map(|project| project.role) == Some(FormProjectRole::Main));
    check!(first_copy.sign_in_required);

    // Orrery named both the project's main form: the first is.
    let second_copy = load(&pool, &FormLookup::Orrery(138))
        .await
        .unwrap()
        .unwrap();
    check!(
        second_copy.project.as_ref().map(|project| project.role) == Some(FormProjectRole::Extra)
    );
    check!(second_copy.closed);

    // Nothing here can draw a page orrery drew itself, until it is seeded.
    let drawn_copy = load(&pool, &FormLookup::Orrery(17)).await.unwrap().unwrap();
    check!(drawn_copy.closed);
    check!(drawn_copy.fields.is_empty());

    // A seeded one is drawn from its seed, and keeps orrery's name.
    let seeded_copy = load(&pool, &FormLookup::Orrery(5)).await.unwrap().unwrap();
    check!(!seeded_copy.closed);
    check!(seeded_copy.name == "Form 5");
    check!(seeded_copy.field("gray_squirrel").is_some());
    check!(seeded_copy.map.is_some());

    // The slug passes to the other form, and the main form with it; the
    // first form is copied again under the id it had, and the one orrery
    // dropped goes.
    counted.slug = Some("sunflowers".to_string());
    counted.main_for = Vec::new();
    counted.entity_ulid = Some(SQUIRREL.to_string());
    second.slug = Some("bees".to_string());
    second.main_for = vec![SQUIRREL.to_string()];
    mirror::mirror_forms(&pool, &[second.clone(), counted.clone()])
        .await
        .unwrap();

    let first_again = load(&pool, &FormLookup::Orrery(137))
        .await
        .unwrap()
        .unwrap();
    check!(first_again.form_id == first_copy.form_id);
    check!(first_again.slug.as_deref() == Some("sunflowers"));
    check!(
        first_again.project.as_ref().map(|project| project.role) == Some(FormProjectRole::Extra)
    );

    let second_again = load(&pool, &FormLookup::Orrery(138))
        .await
        .unwrap()
        .unwrap();
    check!(second_again.slug.as_deref() == Some("bees"));
    check!(
        second_again.project.as_ref().map(|project| project.role) == Some(FormProjectRole::Main)
    );

    check!(load(&pool, &FormLookup::Orrery(17)).await.unwrap() == None);
    check!(load(&pool, &FormLookup::Orrery(5)).await.unwrap() == None);

    // Copying what has not changed changes nothing.
    mirror::mirror_forms(&pool, &[second, counted])
        .await
        .unwrap();
    let unchanged = load(&pool, &FormLookup::Orrery(137))
        .await
        .unwrap()
        .unwrap();
    check!(unchanged.updated == first_again.updated);

    check!(mirror::mirror_forms(&pool, &[]).await.is_err());
}

fn orrery_submission(id: i64, form_id: i64, data_json: &str) -> OrrerySubmission {
    OrrerySubmission {
        id,
        form_id,
        when: "2026-10-25T14:00:00.000000Z".to_string(),
        submitter_ulid: Some(ADA.to_string()),
        final_: true,
        data_json: data_json.to_string(),
        evaluation: String::new(),
        comment: String::new(),
        changed: "2026-10-25T14:00:00.000000Z".to_string(),
        lon: Some(-121.75),
        lat: Some(38.5),
    }
}

#[tokio::test]
async fn orrerys_submissions_are_copied_with_their_places_told_apart() {
    let pool = fixture().await;

    let located = orrery_form(
        1,
        r#"[{"type": "number", "prompt": "Bees", "extra": ""},
            {"type": "location", "prompt": "Where", "extra": ""}]"#,
    );
    let unlocated = orrery_form(2, r#"[{"type": "number", "prompt": "Bees", "extra": ""}]"#);
    let forms = [located, unlocated];
    mirror::mirror_forms(&pool, &forms).await.unwrap();
    let mut copied = CopiedForms::new(&forms);

    for submission in [
        orrery_submission(
            10,
            1,
            r#"{"Bees": 3, "Where": [-120.5, 37.25], "Old question": "yes"}"#,
        ),
        orrery_submission(11, 1, r#"{"Bees": 4}"#),
        orrery_submission(12, 2, r#"{"Bees": 5}"#),
        OrrerySubmission {
            lon: Some(-97.0),
            lat: Some(38.0),
            ..orrery_submission(13, 2, r#"{"Bees": 6}"#)
        },
        orrery_submission(14, 2, "{}"),
        orrery_submission(15, 2, r#"{"Bees": null, "Notes": ""}"#),
        OrrerySubmission {
            final_: false,
            ..orrery_submission(16, 2, r#"{"Bees": 7}"#)
        },
    ] {
        mirror::mirror_submission(&pool, &mut copied, &submission)
            .await
            .unwrap();
    }

    let by_orrery_id = |records: Vec<common::message::form::SubmissionRecord>| {
        records
            .into_iter()
            .map(|record| (record.orrery_id.unwrap(), record))
            .collect::<BTreeMap<_, _>>()
    };
    let located_copies = by_orrery_id(
        all_submissions(
            &pool,
            load(&pool, &FormLookup::Orrery(1))
                .await
                .unwrap()
                .unwrap()
                .form_id,
        )
        .await,
    );
    let unlocated_copies = by_orrery_id(
        all_submissions(
            &pool,
            load(&pool, &FormLookup::Orrery(2))
                .await
                .unwrap()
                .unwrap()
                .form_id,
        )
        .await,
    );

    let picked = &located_copies[&10];
    check!(picked.answers[&FieldKey::from("Bees")] == Answer::Number(3.0));
    check!(
        picked.answers[&FieldKey::from("Old question")] == Answer::Legacy(serde_json::json!("yes"))
    );
    check!(
        picked.location
            == Some(Point {
                latitude: 37.25,
                longitude: -120.5
            })
    );
    check!(picked.ip_location == None);

    // The form asked, and orrery's point is the only place there is.
    check!(
        located_copies[&11].location
            == Some(Point {
                latitude: 38.5,
                longitude: -121.75
            })
    );

    // The form did not ask: orrery's point was where the address was.
    check!(unlocated_copies[&12].location == None);
    check!(
        unlocated_copies[&12].ip_location
            == Some(Point {
                latitude: 38.5,
                longitude: -121.75
            })
    );
    check!(unlocated_copies[&13].ip_location == None);

    // Empty submissions and drafts are not copied.
    check!(unlocated_copies.keys().copied().collect::<Vec<_>>() == vec![12, 13]);

    // A copy is replaced in place, and goes when orrery's does.
    let first_id = picked.submission_id;
    mirror::mirror_submission(
        &pool,
        &mut copied,
        &OrrerySubmission {
            evaluation: "positive".to_string(),
            ..orrery_submission(10, 1, r#"{"Bees": 3}"#)
        },
    )
    .await
    .unwrap();
    mirror::drop_deleted_submissions(&pool, &[10, 12])
        .await
        .unwrap();

    let located_copies = by_orrery_id(
        all_submissions(
            &pool,
            load(&pool, &FormLookup::Orrery(1))
                .await
                .unwrap()
                .unwrap()
                .form_id,
        )
        .await,
    );
    check!(located_copies.keys().copied().collect::<Vec<_>>() == vec![10]);
    check!(located_copies[&10].submission_id == first_id);
    check!(located_copies[&10].evaluation == Some(Verdict::Positive));
}

/// Each seeded form as a definition: its fields and map, as copied.
fn seeded_forms() -> Vec<(i64, FormDefinition)> {
    mirror::SEEDS
        .iter()
        .map(|(orrery_id, json)| {
            let seed: Seed = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("seed for form {orrery_id}: {e}"));
            let mut form = definition(seed.fields);
            form.orrery_id = Some(*orrery_id);
            form.intro = seed.intro;
            form.aside = seed.aside;
            form.map = seed.map;
            (*orrery_id, form)
        })
        .collect()
}

fn seeded_form(orrery_id: i64) -> FormDefinition {
    seeded_forms()
        .into_iter()
        .find(|(id, _)| *id == orrery_id)
        .map(|(_, form)| form)
        .unwrap_or_else(|| panic!("form {orrery_id} is not seeded"))
}

#[test]
fn every_seed_is_a_consistent_definition() {
    let forms = seeded_forms();
    check!(
        forms.iter().map(|(id, _)| *id).collect::<Vec<_>>() == [5, 6, 8, 9, 13],
        "the five forms orrery drew with pages of their own that were in use"
    );

    for (orrery_id, form) in &forms {
        check!(form.inconsistency() == None, "form {orrery_id}");

        // Each page had a map to pick where on, which orrery kept apart from
        // the answers.
        let place = form.field("where");
        check!(
            place.is_some_and(|field| field.kind == FieldKind::Location && field.required),
            "form {orrery_id}"
        );

        // Display items are keyed apart from anything orrery stored.
        for field in form.fields.iter().filter(|field| !field.kind.asks()) {
            check!(
                field.key.as_str().starts_with("heading-")
                    || field.key.as_str().starts_with("note-"),
                "form {orrery_id}: {}",
                field.key
            );
        }
    }
}

/// The keys orrery's pages stored without asking: constants, and inputs
/// since taken off the page. No seeded field claims them.
fn unasked(key: &str) -> bool {
    const CONSTANTS: &[&str] = &[
        "recorder_name",
        "recorder_email",
        "receive_more_info",
        "terms",
        "photo_share",
        "specialized_scistarter_terms_of_use",
        "safety",
    ];

    let day_key = |stem: &str, suffix: &str| {
        key.strip_prefix(stem)
            .and_then(|rest| rest.strip_suffix(suffix))
            .is_some_and(|n| n.parse::<u8>().is_ok())
    };

    CONSTANTS.contains(&key)
        || key.starts_with("optional_bait")
        || day_key("visual_", "_bubbles")
        || day_key("height_after_", "")
        || day_key("ph_fed_", "")
}

/// The answers orrery stored in `data_json` for the seeded form `orrery_id`,
/// as read into that form's fields, having checked that nothing but what its
/// page never asked is left unread.
fn read_stored(orrery_id: i64, data_json: &serde_json::Value) -> common::message::form::Answers {
    let form = seeded_form(orrery_id);
    let answers = mirror::answers_of(&form, data_json);

    for (key, answer) in &answers {
        if matches!(answer, Answer::Legacy(_)) {
            check!(
                unasked(key.as_str()),
                "form {orrery_id}: {key} = {answer:?}"
            );
        } else {
            check!(!unasked(key.as_str()), "form {orrery_id}: {key}");
        }
    }

    answers
}

fn answer<'a>(answers: &'a common::message::form::Answers, key: &str) -> &'a Answer {
    &answers[&FieldKey::from(key)]
}

fn choice(value: &str) -> Answer {
    Answer::Choice(value.to_string())
}

fn written(text: &str) -> Answer {
    Answer::Text(text.to_string())
}

#[test]
fn project_squirrels_stored_answers_read_into_its_fields() {
    let answers = read_stored(
        5,
        &serde_json::json!({
            "recorder_name": "", "recorder_email": "", "receive_more_info": true,
            "terms": false, "gray_squirrel": 1, "red_squirrel": 0, "fox_squirrel": 0,
            "science_photo": "", "mapped": "Yes", "observation_here": "Yes",
            "observation_elsewhere": "Yes", "date": "2026-10-25T04:00:00.000Z",
            "squirrel_time": "07:30", "location_type": "school", "nut_trees": 1,
            "seed_trees": 0, "fruit_trees": 2, "tiny_seed_trees": 0, "conifer_trees": 0,
            "bird_feeder": "never", "human_handout": "never", "garbage": "seldom",
            "from_trees": "often", "other_food": "never", "other_feeding_explain": "",
            "population_change": "decreased", "dog": "often", "cat": "never",
            "coyote": "never", "hawk": "seldom", "poison": "never",
            "comments": "A nest in the oak."
        }),
    );

    check!(*answer(&answers, "gray_squirrel") == Answer::Number(1.0));
    check!(*answer(&answers, "red_squirrel") == Answer::Number(0.0));
    check!(*answer(&answers, "date") == Answer::Date(jiff::civil::date(2026, 10, 25)));
    check!(*answer(&answers, "squirrel_time") == Answer::Time(jiff::civil::time(7, 30, 0, 0)));
    check!(*answer(&answers, "nut_trees") == choice("1"));
    check!(*answer(&answers, "seed_trees") == choice("0"));
    check!(*answer(&answers, "fruit_trees") == choice("2"));
    check!(*answer(&answers, "population_change") == choice("decreased"));
    check!(*answer(&answers, "cat") == choice("never"));
    check!(!answers.contains_key(&FieldKey::from("science_photo")));

    // An unanswered time, and the predators left unchosen.
    let answers = read_stored(
        5,
        &serde_json::json!({
            "recorder_name": "", "recorder_email": "", "receive_more_info": true,
            "terms": false, "gray_squirrel": 0, "red_squirrel": 0, "fox_squirrel": 0,
            "science_photo": "https://orrery-media.s3.amazonaws.com/uploads/2026-11/squirrel.jpg",
            "mapped": "Yes", "observation_here": "No", "observation_elsewhere": "Yes",
            "date": "2026-11-04T21:00:00.000Z", "squirrel_time": "",
            "location_type": "forest_preserve", "nut_trees": 0, "seed_trees": 0,
            "fruit_trees": 0, "tiny_seed_trees": 0, "conifer_trees": 0,
            "bird_feeder": "often", "human_handout": "seldom", "garbage": "seldom",
            "from_trees": "seldom", "other_food": "seldom", "other_feeding_explain": "",
            "population_change": "not sure", "dog": "seldom", "cat": "seldom",
            "coyote": "", "hawk": "", "poison": "", "comments": ""
        }),
    );

    // West of Greenwich, the evening of the 4th.
    check!(*answer(&answers, "date") == Answer::Date(jiff::civil::date(2026, 11, 5)));
    check!(!answers.contains_key(&FieldKey::from("squirrel_time")));
    check!(!answers.contains_key(&FieldKey::from("coyote")));
    check!(
        *answer(&answers, "science_photo")
            == Answer::Photos(vec![
                "https://orrery-media.s3.amazonaws.com/uploads/2026-11/squirrel.jpg".to_string()
            ])
    );
}

#[test]
fn zombee_watchs_stored_answers_read_into_its_fields() {
    let answers = read_stored(
        6,
        &serde_json::json!({
            "photo_share": true, "receive_more_info": true,
            "specialized_scistarter_terms_of_use": false, "sample_name": "Garden sample",
            "method": "d", "mapped": "Yes",
            "bee_submission_date": "2024-06-30T18:30:00.000Z", "bee_count": 1,
            "bee_photo": "https://orrery-media.s3.amazonaws.com/uploads/2026-07/bee.jpg",
            "bee_comments": "One bee.",
            "pupae_submission_date": "2024-06-30T18:30:00.000Z", "pupae_count": 1,
            "pupae_photo": "https://orrery-media.s3.amazonaws.com/uploads/2026-07/pupae.jpg",
            "pupae_comments": "One pupa.",
            "zombie_fly_submission_date": "2024-06-30T18:30:00.000Z",
            "zombie_fly_photo": "", "zombie_fly_comments": "None."
        }),
    );

    // Midnight of the 1st, east of Greenwich.
    check!(*answer(&answers, "bee_submission_date") == Answer::Date(jiff::civil::date(2024, 7, 1)));
    check!(*answer(&answers, "bee_count") == Answer::Number(1.0));
    check!(*answer(&answers, "method") == choice("d"));
    check!(!answers.contains_key(&FieldKey::from("zombie_fly_count")));

    // No bees found: the later stages are left as the page started them.
    let answers = read_stored(
        6,
        &serde_json::json!({
            "photo_share": true, "receive_more_info": true,
            "specialized_scistarter_terms_of_use": false, "sample_name": "Porch light ",
            "method": "t", "mapped": "Yes",
            "bee_submission_date": "2026-08-02T07:00:00.000Z", "bee_count": 0,
            "bee_photo": "https://orrery-media.s3.amazonaws.com/uploads/2026-08/trap.jpg",
            "bee_comments": "", "pupae_submission_date": "", "pupae_count": 0,
            "pupae_photo": "", "pupae_comments": "", "zombie_fly_submission_date": "",
            "zombie_fly_count": 0, "zombie_fly_photo": "", "zombie_fly_comments": ""
        }),
    );

    check!(*answer(&answers, "bee_count") == Answer::Number(0.0));
    check!(*answer(&answers, "pupae_count") == Answer::Number(0.0));
    check!(!answers.contains_key(&FieldKey::from("pupae_submission_date")));
}

#[test]
fn stream_selfies_stored_answers_read_into_its_fields() {
    let answers = read_stored(
        8,
        &serde_json::json!({
            "recorder_name": "", "recorder_email": "", "photo_share": true,
            "receive_more_info": true, "specialized_scistarter_terms_of_use": false,
            "science_photo": "https://orrery-media.s3.amazonaws.com/uploads/2026-10/creek.jpg",
            "science_caption": "Mill Creek", "name": "Mill Creek", "stream_unknown": false,
            "mapped": "Yes", "safety": "", "ownership": "Public", "trash": "None",
            "continuous": "Unsure", "existing_site": "No", "membership": "No",
            "organization": "", "why_matters": "It's close to home"
        }),
    );

    check!(*answer(&answers, "stream_unknown") == Answer::Bool(false));
    check!(*answer(&answers, "trash") == choice("None"));
    check!(*answer(&answers, "name") == written("Mill Creek"));

    let answers = read_stored(
        8,
        &serde_json::json!({
            "recorder_name": "", "recorder_email": "", "photo_share": true,
            "receive_more_info": true, "specialized_scistarter_terms_of_use": false,
            "science_photo": "https://orrery-media.s3.amazonaws.com/uploads/2026-11/stream.jpg",
            "science_caption": "", "name": "", "stream_unknown": true, "mapped": "Yes",
            "safety": "", "ownership": "Don't know", "trash": "None",
            "continuous": "Unsure", "existing_site": "N/A", "membership": "Yes",
            "organization": "A watershed group", "why_matters": "It's beautiful."
        }),
    );

    check!(*answer(&answers, "stream_unknown") == Answer::Bool(true));
    check!(*answer(&answers, "ownership") == choice("Don't know"));
    check!(*answer(&answers, "existing_site") == choice("N/A"));
    check!(*answer(&answers, "organization") == written("A watershed group"));
}

#[test]
fn ant_picnics_stored_answers_read_into_its_fields() {
    let answers = read_stored(
        9,
        &serde_json::json!({
            "school": "Troop 1", "mapped": "Yes", "date": "2026-10-12T04:00:00.000Z",
            "start_time": "15:00", "end_time": "16:00", "temperature": 20.5,
            "degree_type": "C", "cloud": "Other", "cloud_other": "Foggy", "wind": "Light Wind",
            "shade": "Partially", "site_description": "Rock",
            "ant_attractions": "Trash can ", "amino_acids_pre": "0",
            "amino_acids_final": "0", "cookies_pre": "0", "cookies_final": "2.5",
            "olive_oil_pre": "0", "olive_oil_final": "0", "salt_water_pre": "",
            "salt_water_final": "1", "sugar_water_pre": "0", "sugar_water_final": "0",
            "plain_water_pre": "0", "plain_water_final": "0", "optional_bait": "",
            "optional_bait_pre": "", "optional_bait_final": "", "science_photo": "",
            "science_caption": "", "comments": "It had rained the day before."
        }),
    );

    check!(*answer(&answers, "date") == Answer::Date(jiff::civil::date(2026, 10, 12)));
    check!(*answer(&answers, "start_time") == Answer::Time(jiff::civil::time(15, 0, 0, 0)));
    check!(*answer(&answers, "end_time") == Answer::Time(jiff::civil::time(16, 0, 0, 0)));
    check!(*answer(&answers, "temperature") == Answer::Number(20.5));
    check!(*answer(&answers, "cookies_final") == Answer::Number(2.5));
    check!(*answer(&answers, "salt_water_final") == Answer::Number(1.0));
    check!(!answers.contains_key(&FieldKey::from("salt_water_pre")));
    check!(*answer(&answers, "cloud_other") == written("Foggy"));
}

#[test]
fn sourdough_for_sciences_stored_answers_read_into_its_fields() {
    let mut data = serde_json::json!({
        "photo_share": false, "mapped": "Yes", "flour_type": "Other -> Specify",
        "flour_type_other": "Teff and rye", "flour_brand": "A store brand",
        "other_changes": "", "radius": "7.2", "hypothesis": "It will rise.",
        "experiment_photo": "https://orrery-media.s3.amazonaws.com/uploads/2026-04/sheet.png",
        "other_photos": [
            "https://orrery-media.s3.amazonaws.com/uploads/2026-04/jar1.jpg",
            "https://orrery-media.s3.amazonaws.com/uploads/2026-04/jar2.jpg"
        ]
    });

    // The page stored every day, filled in or not; its temperatures start
    // at 0.
    let days = data.as_object_mut().unwrap();
    for n in 0..16 {
        let filled = n < 2;
        for (key, value) in [
            (
                "date",
                serde_json::json!(if filled {
                    "2026-03-19T04:00:00.000Z"
                } else {
                    ""
                }),
            ),
            (
                "height_before",
                serde_json::json!(if filled { "1.2" } else { "" }),
            ),
            ("visual_{n}_bubbles", serde_json::json!("")),
            (
                "visual",
                serde_json::json!(if filled { "Bubbly" } else { "" }),
            ),
            ("temp", serde_json::json!(if filled { 72 } else { 0 })),
            (
                "temp_{n}_units",
                serde_json::json!(if filled { "f" } else { "" }),
            ),
            (
                "aroma",
                serde_json::json!(if filled { "Fruity: Apple" } else { "" }),
            ),
            (
                "ph_mixed",
                serde_json::json!(if filled { "5.0" } else { "" }),
            ),
            ("height_after", serde_json::json!("")),
            ("ph_fed", serde_json::json!("")),
        ] {
            let key = if key.contains("{n}") {
                key.replace("{n}", &n.to_string())
            } else {
                format!("{key}_{n}")
            };
            days.insert(key, value);
        }
    }

    let answers = read_stored(13, &data);

    check!(*answer(&answers, "flour_type") == choice("Other -> Specify"));
    check!(*answer(&answers, "flour_type_other") == written("Teff and rye"));
    check!(*answer(&answers, "radius") == written("7.2"));
    check!(*answer(&answers, "date_1") == Answer::Date(jiff::civil::date(2026, 3, 19)));
    check!(*answer(&answers, "temp_1") == Answer::Number(72.0));
    check!(*answer(&answers, "temp_1_units") == choice("f"));
    check!(*answer(&answers, "temp_15") == Answer::Number(0.0));
    check!(*answer(&answers, "aroma_0") == choice("Fruity: Apple"));
    check!(*answer(&answers, "ph_mixed_0") == written("5.0"));
    check!(!answers.contains_key(&FieldKey::from("date_2")));
    check!(
        *answer(&answers, "other_photos")
            == Answer::Photos(vec![
                "https://orrery-media.s3.amazonaws.com/uploads/2026-04/jar1.jpg".to_string(),
                "https://orrery-media.s3.amazonaws.com/uploads/2026-04/jar2.jpg".to_string(),
            ])
    );
}
