use anyhow::Error;
use async_nats::service::Request;
use common::{
    badge::Badge,
    message::dashboard::{
        AmbassadorAppStatus, AmbassadorApplication, AmbassadorApplicationForm,
        AmbassadorApplicationRow, AmbassadorApplicationUpdate, AmbassadorDirectoryEntry,
        AmbassadorDirectoryOrder, AmbassadorDirectoryQuery, AmbassadorDirectoryResponse,
        AmbassadorMessageForm, AmbassadorMessageReview, AmbassadorMessageRow, AmbassadorProfile,
        AmbassadorReviewUpdate, AmbassadorScreeningStatus,
    },
    message::email::EmailRecipient,
    message::user::{AmbassadorLocation, AmbassadorPassedLocationsResponse, UserAndBadge},
    service::{TriggerEvent, decode_structured_message, send, send_email, trigger_email},
    shared::db,
    timestring::TimeString,
    ulid::AsUlid,
};
use ulid::Ulid;

/// The current native ambassador state-machine state (orrery's
/// `AmbassadorState.state`), defaulting to `initial` for users with no record.
async fn current_state(user_id: &str) -> Result<String, Error> {
    Ok(sqlx::query_scalar!(
        "SELECT state FROM ambassador_progress WHERE user_id = ?",
        user_id
    )
    .fetch_optional(db())
    .await?
    .unwrap_or_else(|| "initial".to_string()))
}

/// Set the native ambassador state, creating the progress row if needed.
async fn set_state(user_id: &str, state: &str) -> Result<(), Error> {
    sqlx::query!(
        "INSERT OR IGNORE INTO ambassador_progress (user_id) VALUES (?)",
        user_id
    )
    .execute(db())
    .await?;
    sqlx::query!(
        "UPDATE ambassador_progress SET state = ? WHERE user_id = ?",
        state,
        user_id
    )
    .execute(db())
    .await?;
    Ok(())
}

/// Advance the native ambassador state to `to`, but only when the user is
/// currently in one of the `from` states. Returns whether the state changed —
/// the caller uses this to fire an entry email exactly once per real transition.
pub async fn advance_state_if(user_id: &str, from: &[&str], to: &str) -> Result<bool, Error> {
    let current = current_state(user_id).await?;
    if from.contains(&current.as_str()) {
        set_state(user_id, to).await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Minimal HTML escaping for user-supplied text interpolated into email bodies.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Distinct city/state locations of ambassadors in the public directory (i.e.
/// who reached the "passed" state, recorded as a non-null `users.ambassador`
/// timestamp). Backs /api/ambassador_map.geojson; the frontend geocodes each
/// location for the map.
pub async fn passed_locations(_req: &Request) -> Result<AmbassadorPassedLocationsResponse, Error> {
    let rows = sqlx::query!(
        r#"SELECT DISTINCT a.city AS "city!", a.state AS "state!"
           FROM ambassador_applications a
           JOIN users u ON u.user_id = a.user_id
           WHERE u.ambassador IS NOT NULL AND a.city != ''
           ORDER BY a.city, a.state"#,
    )
    .fetch_all(db())
    .await?;

    let locations = rows
        .into_iter()
        .map(|r| AmbassadorLocation {
            city: r.city,
            state: r.state,
        })
        .collect();

    Ok(AmbassadorPassedLocationsResponse { locations })
}

pub async fn get(req: &Request) -> Result<Option<AmbassadorApplication>, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;
    let user_ulid = user_id.as_ulid()?;

    let row = sqlx::query!(
        r#"SELECT
            "bio", "city", "state", "contact_email", "youtube_video_url",
            "phone", "tee_size", "self_description", "handles", "experience",
            "motivation", "related", "vision", "community_access", "introduction",
            "introduction_other", "questions", "signature_name", "gender", "ethnicity",
            "activities", "citscimonth", "background_check", "conduct", "birthdate",
            "signature_date", "draft", "app_status", "submitted", "updated_at"
        FROM "ambassador_applications"
        WHERE "user_id" = ?"#,
        user_id
    )
    .fetch_optional(db())
    .await?;

    Ok(row.map(|row| AmbassadorApplication {
        user_id: user_ulid,
        bio: row.bio,
        city: row.city,
        state: row.state,
        contact_email: row.contact_email,
        youtube_video_url: row.youtube_video_url,
        phone: row.phone,
        tee_size: row.tee_size,
        self_description: row.self_description,
        handles: row.handles,
        experience: row.experience,
        motivation: row.motivation,
        related: row.related,
        vision: row.vision,
        community_access: row.community_access,
        introduction: row.introduction,
        introduction_other: row.introduction_other,
        questions: row.questions,
        signature_name: row.signature_name,
        gender: row.gender,
        ethnicity: row.ethnicity,
        activities: row.activities,
        citscimonth: row.citscimonth != 0,
        background_check: row.background_check != 0,
        conduct: row.conduct != 0,
        birthdate: row.birthdate,
        signature_date: row.signature_date,
        draft: row.draft != 0,
        app_status: AmbassadorAppStatus::from_i64(row.app_status),
        submitted: row.submitted,
        updated_at: row.updated_at,
    }))
}

/// Persist a full application (public apply form). `draft = true` for a lax
/// autosave; `draft = false` for final submission, which additionally stamps
/// `submitted` and tags the user. Applicant fields land on the application row;
/// identity/address fields land on the user record — mirroring orrery's
/// `_write_ambassador_application_fields` / `_write_ambassador_user_fields`.
async fn write_application(form: &AmbassadorApplicationForm, draft: bool) -> Result<(), Error> {
    let user_id_str = form.user_id.to_string();

    sqlx::query!(
        r#"INSERT INTO "ambassador_applications" (
            "user_id", "phone", "city", "state", "tee_size", "self_description",
            "contact_email", "handles", "experience", "motivation", "related",
            "vision", "community_access", "introduction", "introduction_other",
            "questions", "signature_name", "gender", "ethnicity", "activities",
            "citscimonth", "background_check", "conduct", "birthdate",
            "signature_date", "draft", "submitted", "updated_at"
        ) VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
            CASE WHEN ? = 1 THEN NULL ELSE current_timestamp END, current_timestamp
        )
        ON CONFLICT ("user_id") DO UPDATE SET
            "phone" = excluded."phone",
            "city" = excluded."city",
            "state" = excluded."state",
            "tee_size" = excluded."tee_size",
            "self_description" = excluded."self_description",
            "contact_email" = excluded."contact_email",
            "handles" = excluded."handles",
            "experience" = excluded."experience",
            "motivation" = excluded."motivation",
            "related" = excluded."related",
            "vision" = excluded."vision",
            "community_access" = excluded."community_access",
            "introduction" = excluded."introduction",
            "introduction_other" = excluded."introduction_other",
            "questions" = excluded."questions",
            "signature_name" = excluded."signature_name",
            "gender" = excluded."gender",
            "ethnicity" = excluded."ethnicity",
            "activities" = excluded."activities",
            "citscimonth" = excluded."citscimonth",
            "background_check" = excluded."background_check",
            "conduct" = excluded."conduct",
            "birthdate" = excluded."birthdate",
            "signature_date" = excluded."signature_date",
            "draft" = ?,
            "submitted" = CASE WHEN ? = 1 THEN "ambassador_applications"."submitted"
                               ELSE COALESCE("ambassador_applications"."submitted", current_timestamp) END,
            "updated_at" = current_timestamp"#,
        user_id_str,
        form.phone,
        form.city,
        form.state,
        form.tee_size,
        form.self_description,
        form.contact_email,
        form.handles,
        form.experience,
        form.motivation,
        form.related,
        form.vision,
        form.community_access,
        form.introduction,
        form.introduction_other,
        form.questions,
        form.signature_name,
        form.gender,
        form.ethnicity,
        form.activities,
        form.citscimonth as i64,
        form.background_check as i64,
        form.conduct as i64,
        form.birthdate,
        form.signature_date,
        draft as i64,      // INSERT: draft column
        draft as i64,      // INSERT: submitted CASE (NULL while drafting)
        draft as i64,      // UPDATE: draft column
        draft as i64,      // UPDATE: submitted CASE (keep while drafting, else stamp)
    )
    .execute(db())
    .await?;

    // Identity + mailing address land on the user record. Only rewrite the
    // composite mailing address when the whole address is present, so a partial
    // draft can't clobber an existing address (mirrors orrery).
    let mailing_address = if !form.address1.trim().is_empty()
        && !form.city.trim().is_empty()
        && !form.state.trim().is_empty()
        && !form.zip.trim().is_empty()
    {
        Some(format!(
            "{}\n{}\n{}\n{}\n{}",
            form.address1, form.address2, form.city, form.state, form.zip
        ))
    } else {
        None
    };
    let zip = (!form.zip.trim().is_empty()).then(|| form.zip.clone());

    sqlx::query!(
        r#"UPDATE "users" SET
            "first_name" = ?,
            "last_name" = ?,
            "mailing_address" = COALESCE(?, "mailing_address"),
            "zipcode" = COALESCE(?, "zipcode"),
            "birthday" = COALESCE(?, "birthday")
        WHERE "user_id" = ?"#,
        form.first_name,
        form.last_name,
        mailing_address,
        zip,
        form.birthdate,
        user_id_str,
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Lax draft autosave of the apply form (`user.ambassador.save_draft`).
pub async fn save_draft(req: &Request) -> Result<(), Error> {
    let form: AmbassadorApplicationForm = decode_structured_message(req.message.clone())?;
    write_application(&form, true).await?;
    Ok(())
}

/// Final submission of the apply form (`user.ambassador.submit`). Persists with
/// `draft = false`, stamps `submitted`, tags the user, and — mirroring orrery's
/// `AmbassadorState.application_enter` — advances the native state to
/// `application` and sends the confirmation email exactly once. Validation
/// happens in the frontend handler before this call.
pub async fn submit(req: &Request) -> Result<(), Error> {
    let form: AmbassadorApplicationForm = decode_structured_message(req.message.clone())?;
    let user_id = form.user_id;
    let user_id_str = user_id.to_string();
    write_application(&form, false).await?;
    send(
        "user.tags.add",
        (
            user_id_str.clone(),
            "scistarter-ambassador-application-submitted".to_string(),
        ),
    )
    .await?;

    // Entering the `application` state from Champion/initial is a genuine
    // first-time submission — send the confirmation email once. A resubmission
    // (already in `application` or later) does not re-fire it.
    if advance_state_if(&user_id_str, &["initial", "tutorial"], "application").await? {
        trigger_email(TriggerEvent::AmbassadorApplicationSubmitted, user_id, None).await?;
    }
    Ok(())
}

pub async fn update(req: &Request) -> Result<(), Error> {
    let update: AmbassadorApplicationUpdate = decode_structured_message(req.message.clone())?;
    let user_id_str = update.user_id.to_string();

    sqlx::query!(
        r#"INSERT INTO "ambassador_applications"
            ("user_id", "bio", "city", "state", "contact_email", "youtube_video_url", "updated_at")
        VALUES (?, ?, ?, ?, ?, ?, current_timestamp)
        ON CONFLICT ("user_id") DO UPDATE SET
            "bio" = excluded."bio",
            "city" = excluded."city",
            "state" = excluded."state",
            "contact_email" = excluded."contact_email",
            "youtube_video_url" = excluded."youtube_video_url",
            "updated_at" = current_timestamp"#,
        user_id_str,
        update.bio,
        update.city,
        update.state,
        update.contact_email,
        update.youtube_video_url,
    )
    .execute(db())
    .await?;

    Ok(())
}

const DIRECTORY_PAGE_SIZE: i64 = 24;

struct DirectoryRow {
    user_id: String,
    first_name: String,
    last_name: String,
    image: Option<String>,
    city: String,
    state: String,
}

impl From<DirectoryRow> for AmbassadorDirectoryEntry {
    fn from(r: DirectoryRow) -> Self {
        AmbassadorDirectoryEntry {
            user_id: r.user_id.as_ulid().unwrap_or_default(),
            name: format!("{} {}", r.first_name, r.last_name)
                .trim()
                .to_string(),
            city: r.city,
            state: r.state,
            image: r.image,
        }
    }
}

/// Public ambassador directory (`user.ambassador.directory`): everyone in the
/// directory (non-null `users.ambassador`, not hidden), filtered by name/near
/// and paginated. Built with a runtime query because the ordering and filters
/// are dynamic.
pub async fn directory(req: &Request) -> Result<AmbassadorDirectoryResponse, Error> {
    let q: AmbassadorDirectoryQuery = decode_structured_message(req.message.clone())?;
    let page = q.page.max(1);
    let offset = (page - 1) * DIRECTORY_PAGE_SIZE;

    let name_empty = q.name.trim().is_empty() as i64;
    let name_like = format!("%{}%", q.name.trim());
    let near_empty = q.near.trim().is_empty() as i64;
    let near_like = format!("%{}%", q.near.trim());
    // "time" sorts newest-certified first; anything else is alphabetical.
    let order = match q.order {
        AmbassadorDirectoryOrder::Time => "time",
        _ => "name",
    };

    let total = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM ambassador_applications a
           JOIN users u ON u.user_id = a.user_id
           WHERE u.ambassador IS NOT NULL AND a.hidden = 0
           AND (? = 1 OR u.first_name LIKE ? OR u.last_name LIKE ? OR a.bio LIKE ?)
           AND (? = 1 OR a.city LIKE ? OR a.state LIKE ? OR u.zipcode LIKE ?)"#,
        name_empty,
        name_like,
        name_like,
        name_like,
        near_empty,
        near_like,
        near_like,
        near_like,
    )
    .fetch_one(db())
    .await? as i64;

    // The ORDER BY switches on a bound param: for "time" the first sort key is
    // the certification timestamp (DESC); otherwise it is NULL for every row, so
    // ordering falls through to name.
    let rows = sqlx::query_as!(
        DirectoryRow,
        r#"SELECT a.user_id AS "user_id!", u.first_name AS "first_name!",
                  u.last_name AS "last_name!", u.image AS "image",
                  a.city AS "city!", a.state AS "state!"
           FROM ambassador_applications a
           JOIN users u ON u.user_id = a.user_id
           WHERE u.ambassador IS NOT NULL AND a.hidden = 0
           AND (? = 1 OR u.first_name LIKE ? OR u.last_name LIKE ? OR a.bio LIKE ?)
           AND (? = 1 OR a.city LIKE ? OR a.state LIKE ? OR u.zipcode LIKE ?)
           ORDER BY (CASE WHEN ? = 'time' THEN u.ambassador END) DESC,
                    u.last_name COLLATE NOCASE, u.first_name COLLATE NOCASE
           LIMIT ? OFFSET ?"#,
        name_empty,
        name_like,
        name_like,
        name_like,
        near_empty,
        near_like,
        near_like,
        near_like,
        order,
        DIRECTORY_PAGE_SIZE,
        offset,
    )
    .fetch_all(db())
    .await?;

    let pages = (total + DIRECTORY_PAGE_SIZE - 1) / DIRECTORY_PAGE_SIZE;

    Ok(AmbassadorDirectoryResponse {
        page,
        pages,
        total,
        entries: rows.into_iter().map(Into::into).collect(),
    })
}

/// A single public ambassador profile (`user.ambassador.profile`). Returns
/// `None` if the user is not a directory ambassador.
pub async fn profile(req: &Request) -> Result<Option<AmbassadorProfile>, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;
    let user_ulid = user_id.as_ulid()?;

    let row = sqlx::query!(
        r#"SELECT u.first_name AS "first_name!", u.last_name AS "last_name!",
                  u.image AS "image", a.bio AS "bio!", a.city AS "city!",
                  a.state AS "state!", a.youtube_video_url AS "youtube_video_url!"
           FROM users u JOIN ambassador_applications a ON a.user_id = u.user_id
           WHERE u.user_id = ? AND u.ambassador IS NOT NULL AND a.hidden = 0"#,
        user_id
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let others = sqlx::query_as!(
        DirectoryRow,
        r#"SELECT a.user_id AS "user_id!", u.first_name AS "first_name!",
                  u.last_name AS "last_name!", u.image AS "image",
                  a.city AS "city!", a.state AS "state!"
           FROM ambassador_applications a JOIN users u ON u.user_id = a.user_id
           WHERE u.ambassador IS NOT NULL AND a.hidden = 0 AND a.user_id != ?
           ORDER BY u.ambassador DESC LIMIT 5"#,
        user_id,
    )
    .fetch_all(db())
    .await?;

    Ok(Some(AmbassadorProfile {
        user_id: user_ulid,
        name: format!("{} {}", row.first_name, row.last_name)
            .trim()
            .to_string(),
        image: row.image,
        city: row.city,
        state: row.state,
        bio: row.bio,
        youtube_video_url: row.youtube_video_url,
        others: others.into_iter().map(Into::into).collect(),
    }))
}

/// All ambassador applications, as rows for the staff processing grid
/// (`user.ambassador.list`). Joined with the user record for identity/handle
/// and the derived directory state (`users.ambassador`).
pub async fn list(_req: &Request) -> Result<Vec<AmbassadorApplicationRow>, Error> {
    let rows = sqlx::query!(
        r#"SELECT
            a.user_id AS "user_id!",
            u.first_name AS "first_name!",
            u.last_name AS "last_name!",
            u.handle AS "handle!",
            u.ambassador AS "ambassador",
            a.contact_email AS "contact_email!",
            a.city AS "city!",
            a.state AS "state!",
            a.phone AS "phone!",
            a.birthdate AS "birthdate",
            a.tee_size AS "tee_size!",
            a.gender AS "gender!",
            a.ethnicity AS "ethnicity!",
            a.marked AS "marked!",
            a.archived AS "archived!",
            a.hidden AS "hidden!",
            a.draft AS "draft!",
            a.cohort AS "cohort!",
            a.app_status AS "app_status!",
            a.screening_status AS "screening_status!",
            a.verified AS "verified",
            a.rejection_sent AS "rejection_sent!",
            a.welcome_sent AS "welcome_sent!",
            a.orientation AS "orientation",
            a.kit_sent AS "kit_sent",
            a.survey AS "survey!",
            a.handles AS "handles!",
            a.reviewers AS "reviewers!",
            a.notes AS "notes!",
            a.submitted AS "submitted",
            a.updated_at AS "updated_at!"
        FROM "ambassador_applications" a
        JOIN "users" u ON u.user_id = a.user_id
        ORDER BY a.submitted DESC"#,
    )
    .fetch_all(db())
    .await?;

    let applications = rows
        .into_iter()
        .map(|row| AmbassadorApplicationRow {
            user_id: row.user_id.as_ulid().unwrap_or_default(),
            name: format!("{} {}", row.first_name, row.last_name)
                .trim()
                .to_string(),
            handle: row.handle,
            email: row.contact_email,
            city: row.city,
            state: row.state,
            phone: row.phone,
            birthdate: row.birthdate,
            tee_size: row.tee_size,
            gender: row.gender,
            ethnicity: row.ethnicity,
            in_directory: row.ambassador.is_some(),
            submitted: row.submitted,
            updated_at: row.updated_at,
            marked: row.marked != 0,
            archived: row.archived != 0,
            hidden: row.hidden != 0,
            draft: row.draft != 0,
            cohort: row.cohort,
            app_status: AmbassadorAppStatus::from_i64(row.app_status),
            screening_status: AmbassadorScreeningStatus::from_i64(row.screening_status),
            verified: row.verified,
            rejection_sent: row.rejection_sent != 0,
            welcome_sent: row.welcome_sent != 0,
            orientation: row.orientation,
            kit_sent: row.kit_sent,
            survey: row.survey != 0,
            handles: row.handles,
            reviewers: row.reviewers,
            notes: row.notes,
        })
        .collect();

    Ok(applications)
}

/// Apply a staff review edit (`user.ambassador.review`) and drive the native
/// ambassador state machine. Persists the editable fields, then reconciles the
/// Certified Ambassador badge and public-directory membership.
pub async fn review(req: &Request) -> Result<(), Error> {
    let update: AmbassadorReviewUpdate = decode_structured_message(req.message.clone())?;
    let user_id_str = update.user_id.to_string();

    sqlx::query!(
        r#"UPDATE "ambassador_applications" SET
            "marked" = ?, "archived" = ?, "hidden" = ?, "draft" = ?, "cohort" = ?,
            "app_status" = ?, "screening_status" = ?, "verified" = ?,
            "rejection_sent" = ?, "welcome_sent" = ?, "orientation" = ?, "kit_sent" = ?,
            "survey" = ?, "handles" = ?, "reviewers" = ?, "notes" = ?,
            "updated_at" = current_timestamp
        WHERE "user_id" = ?"#,
        update.marked as i64,
        update.archived as i64,
        update.hidden as i64,
        update.draft as i64,
        update.cohort,
        update.app_status as i64,
        update.screening_status as i64,
        update.verified,
        update.rejection_sent as i64,
        update.welcome_sent as i64,
        update.orientation,
        update.kit_sent,
        update.survey as i64,
        update.handles,
        update.reviewers,
        update.notes,
        user_id_str,
    )
    .execute(db())
    .await?;

    apply_review_effects(
        update.user_id,
        update.app_status,
        update.screening_status,
        update.verified.is_some(),
    )
    .await?;

    Ok(())
}

/// Reconcile the Certified Ambassador badge and public-directory membership
/// from a review edit. Mirrors orrery's `ambassador_application_pre_save`
/// precedence (Champion → demote > Rejected > screening Failed/International >
/// verified) and is idempotent, so it converges with the orrery state sync
/// ([`crate::apply_ambassador_state`]).
///
/// Two distinct outcomes, matching orrery:
/// - **badge** (Certified Ambassador) is earned once the application is
///   `Accepted` and a `verified` date is set (orrery's `verified_enter`);
/// - **directory** membership (orrery's `passed`) additionally requires the
///   background screening to be `Verified`, and is recorded as a non-null
///   `users.ambassador` timestamp.
///
/// Demoting to `Champion` reverts both (orrery's `revert_to_champion`).
async fn apply_review_effects(
    user_id: Ulid,
    app_status: AmbassadorAppStatus,
    screening: AmbassadorScreeningStatus,
    verified: bool,
) -> Result<(), Error> {
    let user_id_str = user_id.to_string();
    let now = TimeString::now();

    let demote = app_status == AmbassadorAppStatus::Champion;
    let blocked = app_status == AmbassadorAppStatus::Rejected
        || matches!(
            screening,
            AmbassadorScreeningStatus::Failed | AmbassadorScreeningStatus::International
        );
    let badge_earned =
        !demote && !blocked && app_status == AmbassadorAppStatus::Accepted && verified;
    let in_directory = badge_earned && screening == AmbassadorScreeningStatus::Verified;

    // Derive the native state-machine target from this review, mirroring orrery's
    // `ambassador_application_pre_save` precedence. `None` means "no terminal
    // transition" — leave the state as it is (e.g. a still-pending application).
    let target_state: Option<&str> = if demote {
        Some("tutorial")
    } else if app_status == AmbassadorAppStatus::Rejected {
        Some("rejected")
    } else if screening == AmbassadorScreeningStatus::International {
        Some("international")
    } else if screening == AmbassadorScreeningStatus::Failed {
        Some("failed")
    } else if in_directory {
        Some("passed")
    } else if verified {
        Some("verified")
    } else {
        None
    };

    // Fire the entry email for a genuine transition into a rejection state, once.
    // The `verified`/`passed` (accepted) email is the Certified Ambassador badge
    // notification, handled below by the `had_badge` guard; `failed` and the
    // Champion demote have no email, matching orrery.
    if let Some(new_state) = target_state {
        let old_state = current_state(&user_id_str).await?;
        if new_state != old_state {
            set_state(&user_id_str, new_state).await?;
            match new_state {
                "rejected" => {
                    trigger_email(TriggerEvent::AmbassadorRejected, user_id, None).await?;
                }
                "international" => {
                    trigger_email(TriggerEvent::AmbassadorInternational, user_id, None).await?;
                }
                _ => {}
            }
        }
    }

    // Was the badge already granted? Drives one-time side-effects (grant email).
    sqlx::query!(
        "INSERT OR IGNORE INTO ambassador_progress (user_id) VALUES (?)",
        user_id_str
    )
    .execute(db())
    .await?;
    let had_badge = sqlx::query_scalar!(
        "SELECT badge_granted_at FROM ambassador_progress WHERE user_id = ?",
        user_id_str
    )
    .fetch_optional(db())
    .await?
    .flatten()
    .is_some();

    if demote {
        // revert_to_champion: revoke the badge and leave the directory. The
        // Champion certificate (tutorial_completed_at) is preserved.
        sqlx::query!(
            "UPDATE ambassador_progress SET badge_granted_at = NULL WHERE user_id = ?",
            user_id_str
        )
        .execute(db())
        .await?;
        send(
            "user.badges.revoke",
            UserAndBadge {
                user_id,
                badge: Badge::SciStarterAmbassador,
            },
        )
        .await?;
    } else if badge_earned {
        sqlx::query!(
            "UPDATE ambassador_progress SET badge_granted_at = COALESCE(badge_granted_at, ?) WHERE user_id = ?",
            now,
            user_id_str
        )
        .execute(db())
        .await?;
        send(
            "user.badges.grant",
            UserAndBadge {
                user_id,
                badge: Badge::SciStarterAmbassador,
            },
        )
        .await?;
        if !had_badge {
            // Native staff action (not a sync), so an email notification is
            // appropriate here.
            trigger_email(TriggerEvent::BadgeAmbassador, user_id, None).await?;
        }
    }

    // Directory membership: set when passed, cleared otherwise (including on
    // demote). Recorded on the user record so it drives the public directory,
    // the map, and the profile "Ambassador" chip.
    if in_directory {
        sqlx::query!(
            "UPDATE users SET ambassador = COALESCE(ambassador, ?) WHERE user_id = ?",
            now,
            user_id_str
        )
        .execute(db())
        .await?;
    } else {
        sqlx::query!(
            "UPDATE users SET ambassador = NULL WHERE user_id = ?",
            user_id_str
        )
        .execute(db())
        .await?;
    }

    Ok(())
}

/// The email address a contact message should be delivered to for a given
/// ambassador: the application contact email, falling back to the account's
/// primary email. Mirrors orrery's `recipient.ambassador_application.email`.
async fn ambassador_email(user_id: &str) -> Result<Option<String>, Error> {
    let row = sqlx::query!(
        r#"SELECT a.contact_email AS "contact_email?", u.primary_email AS "primary_email!"
           FROM users u
           LEFT JOIN ambassador_applications a ON a.user_id = u.user_id
           WHERE u.user_id = ?"#,
        user_id
    )
    .fetch_optional(db())
    .await?;

    Ok(row.map(|r| {
        let contact = r.contact_email.unwrap_or_default();
        if contact.trim().is_empty() {
            r.primary_email
        } else {
            contact
        }
    }))
}

const STAFF_MAILBOX: &str = "ambassador@scistarter.org";

/// Queue a contact-an-ambassador message for staff moderation
/// (`user.ambassador.message_send`) and notify staff that it is waiting. The
/// message is only delivered to the ambassador once staff accept it. Mirrors
/// orrery's `ambassador_send` + `AmbassadorMessage.notify_staff`.
pub async fn message_send(req: &Request) -> Result<(), Error> {
    let form: AmbassadorMessageForm = decode_structured_message(req.message.clone())?;
    let recipient_str = form.recipient_id.to_string();
    let sender_str = form.sender_id.to_string();

    // Only directory ambassadors are contactable.
    let is_ambassador = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM users WHERE user_id = ? AND ambassador IS NOT NULL",
        recipient_str
    )
    .fetch_one(db())
    .await?;
    if is_ambassador == 0 {
        anyhow::bail!("No such ambassador");
    }

    let id = Ulid::new().to_string();
    sqlx::query!(
        r#"INSERT INTO ambassador_messages
            ("id", "sender", "recipient", "first_name", "last_name", "subject", "dates", "body")
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
        id,
        sender_str,
        recipient_str,
        form.first_name,
        form.last_name,
        form.subject,
        form.dates,
        form.body,
    )
    .execute(db())
    .await?;

    // Notify staff to review the queued message.
    let sender_email = sqlx::query_scalar!(
        "SELECT primary_email FROM users WHERE user_id = ?",
        sender_str
    )
    .fetch_optional(db())
    .await?
    .unwrap_or_default();
    let recipient_name = sqlx::query!(
        r#"SELECT first_name AS "first_name!", last_name AS "last_name!"
           FROM users WHERE user_id = ?"#,
        recipient_str
    )
    .fetch_optional(db())
    .await?
    .map(|r| format!("{} {}", r.first_name, r.last_name).trim().to_string())
    .unwrap_or_default();

    let body = format!(
        "<p>A SciStarter message to an ambassador is awaiting staff review before it can be delivered.</p>\
         <p><strong>From:</strong> {first} {last} &lt;{email}&gt;<br>\
         <strong>To:</strong> ambassador {recipient}<br>\
         <strong>Dates (if applicable):</strong> {dates}</p>\
         <blockquote>{message}</blockquote>\
         <p><a href=\"https://scistarter.org/-admin/ambassadors/messages\">Review pending messages (approve or reject)</a></p>",
        recipient = escape_html(&recipient_name),
        first = escape_html(&form.first_name),
        last = escape_html(&form.last_name),
        email = escape_html(&sender_email),
        dates = escape_html(&form.dates),
        message = escape_html(&form.body).replace('\n', "<br>"),
    );

    // Replies from staff go straight to the sender.
    let staff_reply_to = (!sender_email.trim().is_empty()).then(|| vec![sender_email.clone()]);
    send_email(
        vec![EmailRecipient::Email(STAFF_MAILBOX.to_string())],
        "Message pending review: ambassador message",
        body,
        Vec::<String>::new(),
        None,
        staff_reply_to,
    )
    .await?;

    Ok(())
}

/// The staff moderation queue of contact messages (`user.ambassador.message_list`),
/// newest first. Joins in sender/recipient identity for display.
pub async fn message_list(_req: &Request) -> Result<Vec<AmbassadorMessageRow>, Error> {
    let rows = sqlx::query!(
        r#"SELECT
            m.id AS "id!",
            m."when" AS "when!",
            m.sender AS "sender!",
            m.recipient AS "recipient!",
            m.first_name AS "first_name!",
            m.last_name AS "last_name!",
            m.subject AS "subject!",
            m.dates AS "dates!",
            m.body AS "body!",
            m.accepted AS "accepted!",
            m.sent AS "sent",
            s.primary_email AS "sender_email!",
            r.first_name AS "recipient_first!",
            r.last_name AS "recipient_last!",
            COALESCE(NULLIF(ra.contact_email, ''), r.primary_email) AS "recipient_email!: String"
        FROM ambassador_messages m
        JOIN users s ON s.user_id = m.sender
        JOIN users r ON r.user_id = m.recipient
        LEFT JOIN ambassador_applications ra ON ra.user_id = m.recipient
        ORDER BY m."when" DESC"#,
    )
    .fetch_all(db())
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| AmbassadorMessageRow {
            id: row.id.as_ulid().unwrap_or_default(),
            when: row.when,
            sender_id: row.sender.as_ulid().unwrap_or_default(),
            sender_email: row.sender_email,
            recipient_id: row.recipient.as_ulid().unwrap_or_default(),
            recipient_name: format!("{} {}", row.recipient_first, row.recipient_last)
                .trim()
                .to_string(),
            recipient_email: row.recipient_email,
            first_name: row.first_name,
            last_name: row.last_name,
            subject: row.subject,
            dates: row.dates,
            body: row.body,
            accepted: row.accepted != 0,
            sent: row.sent,
        })
        .collect())
}

/// Apply a staff decision to a queued message (`user.ambassador.message_review`).
/// Accepting an un-sent message delivers it: the message is emailed to the
/// ambassador with a copy to the staff mailbox, and stamped `sent` so repeated
/// reviews never re-send. Mirrors orrery's `ambassador_message_post_save` +
/// `AmbassadorMessage.deliver`.
pub async fn message_review(req: &Request) -> Result<(), Error> {
    let review: AmbassadorMessageReview = decode_structured_message(req.message.clone())?;
    let id_str = review.id.to_string();

    sqlx::query!(
        "UPDATE ambassador_messages SET accepted = ? WHERE id = ?",
        review.accepted as i64,
        id_str
    )
    .execute(db())
    .await?;

    if !review.accepted {
        return Ok(());
    }

    // Deliver once: only an accepted, not-yet-sent message.
    let row = sqlx::query!(
        r#"SELECT sender AS "sender!", recipient AS "recipient!",
                  first_name AS "first_name!", last_name AS "last_name!",
                  dates AS "dates!", body AS "body!", sent AS "sent"
           FROM ambassador_messages WHERE id = ?"#,
        id_str
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        return Ok(());
    };
    if row.sent.is_some() {
        return Ok(());
    }

    let Some(recipient_email) = ambassador_email(&row.recipient).await? else {
        anyhow::bail!("Ambassador has no deliverable email address");
    };
    let sender_email = sqlx::query_scalar!(
        "SELECT primary_email FROM users WHERE user_id = ?",
        row.sender
    )
    .fetch_optional(db())
    .await?
    .unwrap_or_default();

    // The sender's email also appears in the body for visibility; replies are
    // routed via the Reply-To header (mirroring orrery's `deliver`).
    let body = format!(
        "<p><strong>Sender name:</strong> {first} {last}<br>\
         <strong>Sender email:</strong> {email}<br>\
         <strong>Dates (if applicable):</strong> {dates}</p>\
         <p><strong>Message:</strong></p><p>{message}</p>",
        first = escape_html(&row.first_name),
        last = escape_html(&row.last_name),
        email = escape_html(&sender_email),
        dates = escape_html(&row.dates),
        message = escape_html(&row.body).replace('\n', "<br>"),
    );

    // Delivered message: the ambassador replies to the sender or the staff
    // mailbox. Staff copy: replies reach the sender or the ambassador.
    let has_sender = !sender_email.trim().is_empty();
    let deliver_reply_to = if has_sender {
        vec![sender_email.clone(), STAFF_MAILBOX.to_string()]
    } else {
        vec![STAFF_MAILBOX.to_string()]
    };
    let copy_reply_to = if has_sender {
        vec![sender_email.clone(), recipient_email.clone()]
    } else {
        vec![recipient_email.clone()]
    };

    send_email(
        vec![EmailRecipient::Email(recipient_email.clone())],
        "SciStarter Ambassador Request",
        body.clone(),
        Vec::<String>::new(),
        None,
        deliver_reply_to,
    )
    .await?;
    send_email(
        vec![EmailRecipient::Email(STAFF_MAILBOX.to_string())],
        "(COPY) SciStarter Ambassador Request",
        format!(
            "<p>ORIGINAL TO {}</p>{}",
            escape_html(&recipient_email),
            body
        ),
        Vec::<String>::new(),
        None,
        copy_reply_to,
    )
    .await?;

    sqlx::query!(
        "UPDATE ambassador_messages SET sent = current_timestamp WHERE id = ?",
        id_str
    )
    .execute(db())
    .await?;

    Ok(())
}
