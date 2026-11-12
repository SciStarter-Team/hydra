//! The forms SciStarter hosts for projects, and what people submit through
//! them (common's `message::form`).
//!
//! A form's definition is kept whole, its fields as JSON, and read back
//! whole: everything a submission is held to is decided by
//! [`FormDefinition::read_submission`], so the store only keeps it.
//!
//! Until cutover both are copies of orrery's ([`mirror`]).

use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::form::{
        FormDefinition, FormDeleted, FormLookup, FormMap, FormProject, FormProjectRole, FormSaved,
        FormSummary,
    },
    primary::CopyKeeper,
    service::decode_structured_message,
    shared::db,
    timestring::TimeString,
    transaction::Immediate,
    ulid::AsUlid,
};
use sqlx::{Executor, Sqlite, SqlitePool};
use ulid::Ulid;

pub mod mirror;
pub mod submissions;

#[cfg(test)]
mod tests;

/// A form as its row holds it.
struct FormRow {
    form_id: String,
    orrery_id: Option<i64>,
    slug: Option<String>,
    name: String,
    intro: String,
    aside: String,
    image: String,
    fields: String,
    closed: bool,
    sign_in_required: bool,
    anonymized: bool,
    credits_participation: bool,
    repetitions_field: Option<String>,
    next_url: String,
    confirmation_message: String,
    first_submission_email: Option<String>,
    project_id: Option<String>,
    project_role: Option<FormProjectRole>,
    map: Option<String>,
    /// JSON: the ids of the form's owners.
    owners: String,
    created: TimeString,
    updated: TimeString,
}

impl TryFrom<FormRow> for FormDefinition {
    type Error = Error;

    fn try_from(row: FormRow) -> Result<Self, Error> {
        let project = match (row.project_id, row.project_role) {
            (Some(project_id), Some(role)) => Some(FormProject { project_id, role }),
            _ => None,
        };

        let map: Option<FormMap> = row.map.as_deref().map(serde_json::from_str).transpose()?;

        Ok(FormDefinition {
            form_id: row.form_id.as_ulid()?,
            orrery_id: row.orrery_id,
            slug: row.slug,
            name: row.name,
            intro: row.intro,
            aside: row.aside,
            image: row.image,
            fields: serde_json::from_str(&row.fields)?,
            closed: row.closed,
            sign_in_required: row.sign_in_required,
            anonymized: row.anonymized,
            credits_participation: row.credits_participation,
            repetitions_field: row.repetitions_field.map(|key| key.as_str().into()),
            next_url: row.next_url,
            confirmation_message: row.confirmation_message,
            first_submission_email: row.first_submission_email,
            project,
            owners: serde_json::from_str(&row.owners)?,
            map,
            created: row.created,
            updated: row.updated,
        })
    }
}

/// The form `lookup` names, if there is one.
pub async fn load<'c>(
    conn: impl Executor<'c, Database = Sqlite>,
    lookup: &FormLookup,
) -> Result<Option<FormDefinition>, Error> {
    let (form_id, orrery_id, slug) = match lookup {
        FormLookup::Id(form_id) => (Some(form_id.to_string()), None, None),
        FormLookup::Orrery(orrery_id) => (None, Some(*orrery_id), None),
        FormLookup::Slug(slug) => (None, None, Some(slug.as_str())),
    };

    let row = sqlx::query_as!(
        FormRow,
        r#"SELECT "form_id" AS "form_id!", "orrery_id", "slug", "name", "intro", "aside",
                  "image", "fields", "closed" AS "closed: bool",
                  "sign_in_required" AS "sign_in_required: bool",
                  "anonymized" AS "anonymized: bool",
                  "credits_participation" AS "credits_participation: bool",
                  "repetitions_field", "next_url", "confirmation_message",
                  "first_submission_email", "project_id",
                  "project_role" AS "project_role: FormProjectRole", "map",
                  (SELECT json_group_array("user_id" ORDER BY "user_id") FROM "form_owners" o
                   WHERE o."form_id" = "forms"."form_id") AS "owners!: String",
                  "created" AS "created: TimeString", "updated" AS "updated: TimeString"
           FROM "forms"
           WHERE "form_id" = ?1 OR "orrery_id" = ?2 OR "slug" = ?3"#,
        form_id,
        orrery_id,
        slug
    )
    .fetch_optional(conn)
    .await?;

    row.map(FormDefinition::try_from).transpose()
}

/// Keep `definition` as it is, in place of whatever was kept under its id,
/// inside the caller's transaction. The caller has checked it fits
/// together and that its slug and its project's main form are its own.
///
/// An owner who is not a user here is left out ([`known_users`]).
async fn store(tx: &mut Immediate, definition: &FormDefinition) -> Result<(), Error> {
    let form_id = definition.form_id.to_string();
    let fields = serde_json::to_string(&definition.fields)?;
    let repetitions_field = definition
        .repetitions_field
        .as_ref()
        .map(|key| key.as_str());
    let (project_id, project_role) = match &definition.project {
        Some(project) => (Some(project.project_id.as_str()), Some(project.role)),
        None => (None, None),
    };
    let map = definition
        .map
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;

    sqlx::query!(
        r#"INSERT INTO "forms"
             ("form_id", "orrery_id", "slug", "name", "intro", "aside", "image", "fields",
              "closed", "sign_in_required", "anonymized", "credits_participation",
              "repetitions_field", "next_url", "confirmation_message", "first_submission_email",
              "project_id", "project_role", "map", "created", "updated")
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT ("form_id") DO UPDATE SET
             "orrery_id" = excluded."orrery_id",
             "slug" = excluded."slug",
             "name" = excluded."name",
             "intro" = excluded."intro",
             "aside" = excluded."aside",
             "image" = excluded."image",
             "fields" = excluded."fields",
             "closed" = excluded."closed",
             "sign_in_required" = excluded."sign_in_required",
             "anonymized" = excluded."anonymized",
             "credits_participation" = excluded."credits_participation",
             "repetitions_field" = excluded."repetitions_field",
             "next_url" = excluded."next_url",
             "confirmation_message" = excluded."confirmation_message",
             "first_submission_email" = excluded."first_submission_email",
             "project_id" = excluded."project_id",
             "project_role" = excluded."project_role",
             "map" = excluded."map",
             "updated" = excluded."updated""#,
        form_id,
        definition.orrery_id,
        definition.slug,
        definition.name,
        definition.intro,
        definition.aside,
        definition.image,
        fields,
        definition.closed,
        definition.sign_in_required,
        definition.anonymized,
        definition.credits_participation,
        repetitions_field,
        definition.next_url,
        definition.confirmation_message,
        definition.first_submission_email,
        project_id,
        project_role,
        map,
        definition.created,
        definition.updated,
    )
    .execute(&mut *tx)
    .await?;

    let owners = serde_json::to_string(&known_users(&mut *tx, &definition.owners).await?)?;
    sqlx::query!(r#"DELETE FROM "form_owners" WHERE "form_id" = ?"#, form_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(
        r#"INSERT OR IGNORE INTO "form_owners" ("form_id", "user_id")
           SELECT ?, "value" FROM json_each(?)"#,
        form_id,
        owners
    )
    .execute(&mut *tx)
    .await?;

    Ok(())
}

/// Those of `users` who are users here, in order.
async fn known_users<'c>(
    conn: impl Executor<'c, Database = Sqlite>,
    users: &[Ulid],
) -> Result<Vec<Ulid>, Error> {
    let users = serde_json::to_string(users)?;

    // The query macros cannot describe a column of `json_each`.
    sqlx::query_scalar::<_, String>(
        r#"SELECT "value" FROM json_each(?)
           WHERE "value" IN (SELECT "user_id" FROM "users")
           ORDER BY "key""#,
    )
    .bind(users)
    .fetch_all(conn)
    .await?
    .iter()
    .map(|user_id| Ok(user_id.as_ulid()?))
    .collect()
}

/// Whether a form other than `form_id` has the slug `slug`.
async fn slug_taken<'c>(
    conn: impl Executor<'c, Database = Sqlite>,
    form_id: &str,
    slug: &str,
) -> Result<bool, Error> {
    Ok(sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM "forms" WHERE "slug" = ? AND "form_id" <> ?)
             AS "taken!: bool""#,
        slug,
        form_id
    )
    .fetch_one(conn)
    .await?)
}

/// Whether a form other than `form_id` is the main form of `project_id`.
async fn main_taken<'c>(
    conn: impl Executor<'c, Database = Sqlite>,
    form_id: &str,
    project_id: &str,
) -> Result<bool, Error> {
    Ok(sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM "forms"
                          WHERE "project_id" = ? AND "project_role" = 'main'
                            AND "form_id" <> ?)
             AS "taken!: bool""#,
        project_id,
        form_id
    )
    .fetch_one(conn)
    .await?)
}

/// Which forms a list of them takes in.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Listed<'a> {
    Every,
    /// Those of the project with this id.
    OfProject(&'a str),
    /// Those this user owns.
    OwnedBy(Ulid),
}

/// What the lists of forms say of each, for the forms `listed`, by name.
pub(crate) async fn summaries(
    pool: &SqlitePool,
    listed: Listed<'_>,
) -> Result<Vec<FormSummary>, Error> {
    let (project_id, owner) = match listed {
        Listed::Every => (None, None),
        Listed::OfProject(project_id) => (Some(project_id), None),
        Listed::OwnedBy(user_id) => (None, Some(user_id.to_string())),
    };

    let rows = sqlx::query!(
        r#"SELECT "form_id" AS "form_id!", "orrery_id", "slug", "name",
                  "closed" AS "closed: bool", "project_id",
                  "project_role" AS "project_role: FormProjectRole"
           FROM "forms"
           WHERE (?1 IS NULL OR "project_id" = ?1)
             AND (?2 IS NULL OR "form_id" IN
                   (SELECT "form_id" FROM "form_owners" WHERE "user_id" = ?2))
           ORDER BY "name" COLLATE NOCASE, "form_id""#,
        project_id,
        owner
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(FormSummary {
                form_id: row.form_id.as_ulid()?,
                orrery_id: row.orrery_id,
                slug: row.slug,
                name: row.name,
                closed: row.closed,
                project: match (row.project_id, row.project_role) {
                    (Some(project_id), Some(role)) => Some(FormProject { project_id, role }),
                    _ => None,
                },
            })
        })
        .collect()
}

/// `user.forms.get`: the form a [`FormLookup`] names, if there is one.
pub async fn get(req: &Request) -> Result<Option<FormDefinition>, Error> {
    let lookup: FormLookup = decode_structured_message(req.message.clone())?;
    load(db(), &lookup).await
}

/// `user.forms.list`: every form.
pub async fn list(_req: &Request) -> Result<Vec<FormSummary>, Error> {
    summaries(db(), Listed::Every).await
}

/// `user.forms.for_project`: the forms of the project whose id is sent.
pub async fn for_project(req: &Request) -> Result<Vec<FormSummary>, Error> {
    let project_id: String = decode_structured_message(req.message.clone())?;

    summaries(db(), Listed::OfProject(&project_id)).await
}

/// `user.forms.owned_by`: the forms the user whose id is sent owns.
pub async fn owned_by(req: &Request) -> Result<Vec<FormSummary>, Error> {
    let user_id: Ulid = decode_structured_message(req.message.clone())?;

    summaries(db(), Listed::OwnedBy(user_id)).await
}

/// `user.forms.save`: keep a form staff made or changed. Its creation time
/// is the kept one, where there is one; its update time is now.
pub async fn save(req: &Request) -> Result<FormSaved, Error> {
    let definition: FormDefinition = decode_structured_message(req.message.clone())?;

    save_definition(db(), definition, CopyKeeper::NOW).await
}

/// Keep `definition`, unless it is a copy `keeper` says orrery keeps: one
/// that has an orrery id, or that replaces a form that has.
pub(crate) async fn save_definition(
    pool: &SqlitePool,
    mut definition: FormDefinition,
    keeper: CopyKeeper,
) -> Result<FormSaved, Error> {
    if let Some(inconsistency) = definition.inconsistency() {
        return Ok(FormSaved::Inconsistent(inconsistency));
    }

    let form_id = definition.form_id.to_string();
    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;
    let kept = load(&mut tx, &FormLookup::Id(definition.form_id)).await?;

    let replaces_copy = kept
        .as_ref()
        .is_some_and(|kept| keeper.orrery_owns(kept.orrery_id));
    if keeper.orrery_owns(definition.orrery_id) || replaces_copy {
        return Ok(FormSaved::KeptByOrrery);
    }

    if let Some(slug) = &definition.slug
        && slug_taken(&mut tx, &form_id, slug).await?
    {
        return Ok(FormSaved::SlugTaken);
    }

    if let Some(project) = &definition.project
        && project.role == FormProjectRole::Main
        && main_taken(&mut tx, &form_id, &project.project_id).await?
    {
        return Ok(FormSaved::MainTaken);
    }

    let now = TimeString::now();
    definition.created = kept.map_or_else(|| now.clone(), |kept| kept.created);
    definition.updated = now;

    store(&mut tx, &definition).await?;
    tx.commit().await?;

    Ok(FormSaved::Saved)
}

/// `user.forms.delete`: delete the form whose id is sent, and every
/// submission of it.
pub async fn delete(req: &Request) -> Result<FormDeleted, Error> {
    let form_id: Ulid = decode_structured_message(req.message.clone())?;

    delete_form(db(), form_id, CopyKeeper::NOW).await
}

/// Delete the form `form_id`, and every submission of it, unless it is a
/// copy `keeper` says orrery keeps.
pub(crate) async fn delete_form(
    pool: &SqlitePool,
    form_id: Ulid,
    keeper: CopyKeeper,
) -> Result<FormDeleted, Error> {
    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    let Some(kept) = load(&mut tx, &FormLookup::Id(form_id)).await? else {
        return Ok(FormDeleted::Missing);
    };

    if keeper.orrery_owns(kept.orrery_id) {
        return Ok(FormDeleted::KeptByOrrery);
    }

    let form_id = form_id.to_string();
    sqlx::query!(r#"DELETE FROM "forms" WHERE "form_id" = ?"#, form_id)
        .execute(&mut tx)
        .await?;
    tx.commit().await?;

    Ok(FormDeleted::Deleted)
}
