use anyhow::Error;
use async_nats::service::Request;
use common::{
    badge::Badge,
    message::dashboard::{
        AmbassadorAppStatus, AmbassadorApplication, AmbassadorApplicationForm,
        AmbassadorApplicationRow, AmbassadorApplicationUpdate, AmbassadorDirectoryEntry,
        AmbassadorDirectoryOrder, AmbassadorDirectoryQuery, AmbassadorDirectoryResponse,
        AmbassadorExportRow, AmbassadorFilters, AmbassadorListRequest, AmbassadorListResponse,
        AmbassadorMessageForm, AmbassadorMessageReview, AmbassadorMessageRow, AmbassadorMilestones,
        AmbassadorProfile, AmbassadorProfileQuery, AmbassadorReviewUpdate,
        AmbassadorScreeningStatus, AmbassadorStage, AmbassadorStageRow, AmbassadorStagesRequest,
        DatePresence, TriState,
    },
    message::email::EmailRecipient,
    message::user::{
        AmbassadorDirectoryListingResponse, AmbassadorLocation, AmbassadorPassedLocationsResponse,
        UserAndBadge,
    },
    service::{TriggerEvent, decode_structured_message, send, send_email, trigger_email},
    shared::db,
    timestring::TimeString,
    transaction::Immediate,
    ulid::AsUlid,
};
use sqlx::{QueryBuilder, Row, Sqlite};
use ulid::Ulid;

/// A way the route past Champion can close, and the milestone that dates it.
///
/// These three are the funnel stages no other table records: an application
/// carries the verdict that produced them, but not when it was reached, and
/// orrery's own state machine dates only its latest move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosedRoute {
    Rejected,
    International,
    ScreeningFailed,
}

impl ClosedRoute {
    /// The closure this stage is, if it is one.
    pub fn of(stage: AmbassadorStage) -> Option<ClosedRoute> {
        match stage {
            AmbassadorStage::Rejected => Some(ClosedRoute::Rejected),
            AmbassadorStage::International => Some(ClosedRoute::International),
            AmbassadorStage::Failed => Some(ClosedRoute::ScreeningFailed),
            AmbassadorStage::Tutorial
            | AmbassadorStage::Application
            | AmbassadorStage::Verified
            | AmbassadorStage::Passed => None,
        }
    }
}

/// Record that the route closed, and say whether this write is what closed it.
///
/// The stamp is the guard as well as the date. An entry email is sent only on
/// the write that first sets it, so a second staff review of the same verdict —
/// or the orrery sync having recorded it first — cannot send the mail twice.
/// This is the same shape the tutorial milestone uses in
/// [`crate::workflow`], and it replaces the stored state machine that used to
/// answer "is this a genuine transition?".
pub async fn close_route(user_id: &str, closed: ClosedRoute, when: &str) -> Result<bool, Error> {
    sqlx::query!(
        "INSERT OR IGNORE INTO ambassador_progress (user_id) VALUES (?)",
        user_id
    )
    .execute(db())
    .await?;

    let result = match closed {
        ClosedRoute::Rejected => {
            sqlx::query!(
                "UPDATE ambassador_progress SET rejected_at = ? WHERE user_id = ? AND rejected_at IS NULL",
                when,
                user_id
            )
            .execute(db())
            .await?
        }
        ClosedRoute::International => {
            sqlx::query!(
                "UPDATE ambassador_progress SET international_at = ? WHERE user_id = ? AND international_at IS NULL",
                when,
                user_id
            )
            .execute(db())
            .await?
        }
        ClosedRoute::ScreeningFailed => {
            sqlx::query!(
                "UPDATE ambassador_progress SET screening_failed_at = ? WHERE user_id = ? AND screening_failed_at IS NULL",
                when,
                user_id
            )
            .execute(db())
            .await?
        }
    };

    Ok(result.rows_affected() > 0)
}

/// Minimal HTML escaping for user-supplied text interpolated into email bodies.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Distinct city/state locations of the ambassadors the public directory
/// lists (the `ambassador_directory_listing` view). Backs
/// /api/ambassador_map.geojson; the frontend geocodes each location for the map.
pub async fn passed_locations(_req: &Request) -> Result<AmbassadorPassedLocationsResponse, Error> {
    let rows = sqlx::query!(
        r#"SELECT DISTINCT l.city AS "city!", l.state AS "state!"
           FROM ambassador_directory_listing l
           WHERE l.city != ''
           ORDER BY l.city, l.state"#,
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

/// Every ambassador the public directory lists (the
/// `ambassador_directory_listing` view), each of whom has a public profile.
/// Backs the sitemap.
pub async fn directory_listing(
    _req: &Request,
) -> Result<AmbassadorDirectoryListingResponse, Error> {
    let user_ids = sqlx::query_scalar!(
        r#"SELECT l.user_id AS "user_id!" FROM ambassador_directory_listing l ORDER BY l.user_id"#,
    )
    .fetch_all(db())
    .await?
    .into_iter()
    .filter_map(|id| id.as_ulid().ok())
    .collect();

    Ok(AmbassadorDirectoryListingResponse { user_ids })
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
/// `AmbassadorState.application_enter` — sends the confirmation email exactly
/// once. Validation happens in the frontend handler before this call.
pub async fn submit(req: &Request) -> Result<(), Error> {
    let form: AmbassadorApplicationForm = decode_structured_message(req.message.clone())?;
    let user_id = form.user_id;
    let user_id_str = user_id.to_string();

    // `submitted` is stamped by the first non-draft write and kept thereafter,
    // so its absence beforehand is what makes this a genuine first submission.
    // A resubmission finds it already set and does not re-fire the email.
    let first_submission = sqlx::query_scalar!(
        r#"SELECT "submitted" FROM "ambassador_applications" WHERE "user_id" = ?"#,
        user_id_str
    )
    .fetch_optional(db())
    .await?
    .flatten()
    .is_none();

    write_application(&form, false).await?;
    send(
        "user.tags.add",
        (
            user_id_str.clone(),
            "scistarter-ambassador-application-submitted".to_string(),
        ),
    )
    .await?;

    if first_submission {
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

/// How many other ambassadors a profile suggests.
const NEARBY_LIMIT: i64 = 5;

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

/// What the directory's "near" box asks for. The three readings are orrery's,
/// so a search that worked there works here.
#[derive(Debug, PartialEq, Eq)]
enum NearFilter {
    Anywhere,
    /// "Sioux Falls, SD": exactly that city in exactly that state.
    CityAndState {
        city: String,
        state: String,
    },
    /// "SD", or anything else typed in capitals: part of a state.
    State(String),
    /// Anything else: part of a city, a state or a ZIP code.
    Loose(String),
}

impl NearFilter {
    fn parse(near: &str) -> NearFilter {
        let near = near.trim();
        if near.is_empty() {
            return NearFilter::Anywhere;
        }
        if let Some((city, state)) = near.split_once(',') {
            return NearFilter::CityAndState {
                city: city.trim().to_string(),
                state: state.trim().to_string(),
            };
        }
        // Python's `str.isupper`, which orrery uses: some letter, and none of
        // them lower case.
        let shouting =
            near.chars().any(char::is_uppercase) && !near.chars().any(char::is_lowercase);
        if shouting {
            NearFilter::State(near.to_string())
        } else {
            NearFilter::Loose(near.to_string())
        }
    }
}

/// A LIKE pattern matching `text` anywhere, with its own `%`, `_` and `\`
/// taken literally. Use with `ESCAPE '\'`.
fn contains_pattern(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len() + 2);
    pattern.push('%');
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

/// Push the directory's search onto a query over `ambassador_directory_listing
/// l JOIN users u` that already ends in a WHERE clause. Shared by the page and
/// its count, so the two cannot disagree about what matched.
fn push_directory_filters(qb: &mut QueryBuilder<Sqlite>, name: &str, near: &NearFilter) {
    // "Jane  Doe" and "Jane Doe" are the same search.
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if !name.is_empty() {
        let pattern = contains_pattern(&name);
        qb.push(" AND (u.first_name LIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" ESCAPE '\\' OR u.last_name LIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" ESCAPE '\\' OR l.bio LIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" ESCAPE '\\' OR (u.first_name || ' ' || u.last_name) LIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" ESCAPE '\\' OR (u.last_name || ' ' || u.first_name) LIKE ");
        qb.push_bind(pattern);
        qb.push(" ESCAPE '\\')");
    }

    match near {
        NearFilter::Anywhere => {}
        NearFilter::CityAndState { city, state } => {
            qb.push(" AND lower(l.city) = lower(");
            qb.push_bind(city.clone());
            qb.push(") AND lower(l.state) = lower(");
            qb.push_bind(state.clone());
            qb.push(")");
        }
        NearFilter::State(state) => {
            qb.push(" AND l.state LIKE ");
            qb.push_bind(contains_pattern(state));
            qb.push(" ESCAPE '\\'");
        }
        NearFilter::Loose(place) => {
            let pattern = contains_pattern(place);
            qb.push(" AND (u.zipcode LIKE ");
            qb.push_bind(pattern.clone());
            qb.push(" ESCAPE '\\' OR l.city LIKE ");
            qb.push_bind(pattern.clone());
            qb.push(" ESCAPE '\\' OR l.state LIKE ");
            qb.push_bind(pattern);
            qb.push(" ESCAPE '\\')");
        }
    }
}

/// Public ambassador directory (`user.ambassador.directory`): everyone
/// `ambassador_directory_listing` lists, searched and paginated.
pub async fn directory(req: &Request) -> Result<AmbassadorDirectoryResponse, Error> {
    let q: AmbassadorDirectoryQuery = decode_structured_message(req.message.clone())?;
    let page = q.page.max(1);
    let offset = (page - 1) * DIRECTORY_PAGE_SIZE;
    let near = NearFilter::parse(&q.near);

    let mut count = QueryBuilder::<Sqlite>::new(
        "SELECT COUNT(*) FROM ambassador_directory_listing l \
         JOIN users u ON u.user_id = l.user_id WHERE 1 = 1",
    );
    push_directory_filters(&mut count, &q.name, &near);
    let total: i64 = count.build_query_scalar().fetch_one(db()).await?;

    let mut select = QueryBuilder::<Sqlite>::new(
        "SELECT l.user_id, u.first_name, u.last_name, u.image, l.city, l.state \
         FROM ambassador_directory_listing l \
         JOIN users u ON u.user_id = l.user_id WHERE 1 = 1",
    );
    push_directory_filters(&mut select, &q.name, &near);
    // The user id breaks ties, so a page boundary never falls between two
    // rows whose relative order the database is free to change.
    select.push(match q.order {
        AmbassadorDirectoryOrder::Random => " ORDER BY l.position IS NULL, l.position, l.user_id",
        AmbassadorDirectoryOrder::Name => {
            " ORDER BY u.last_name COLLATE NOCASE, u.first_name COLLATE NOCASE, l.user_id"
        }
        AmbassadorDirectoryOrder::Time => " ORDER BY u.ambassador DESC, l.user_id",
    });
    select.push(" LIMIT ");
    select.push_bind(DIRECTORY_PAGE_SIZE);
    select.push(" OFFSET ");
    select.push_bind(offset);

    let entries = select
        .build()
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| {
            DirectoryRow {
                user_id: row.get("user_id"),
                first_name: row.get("first_name"),
                last_name: row.get("last_name"),
                image: row.get("image"),
                city: row.get("city"),
                state: row.get("state"),
            }
            .into()
        })
        .collect();

    Ok(AmbassadorDirectoryResponse {
        page,
        pages: (total + DIRECTORY_PAGE_SIZE - 1) / DIRECTORY_PAGE_SIZE,
        total,
        entries,
    })
}

/// One ambassador profile (`user.ambassador.profile`).
///
/// Anyone sees a listed ambassador. The owner and staff also see one who is
/// not listed, provided an application exists to show, marked `listed: false`
/// so the page can say so. Everybody else gets `None`.
pub async fn profile(req: &Request) -> Result<Option<AmbassadorProfile>, Error> {
    let q: AmbassadorProfileQuery = decode_structured_message(req.message.clone())?;
    let user_id = q.user_id.to_string();

    let Some(row) = sqlx::query!(
        r#"SELECT u.first_name AS "first_name!", u.last_name AS "last_name!",
                  u.image AS "image",
                  COALESCE(a.city, '') AS "city!: String",
                  COALESCE(a.state, '') AS "state!: String",
                  COALESCE(a.bio, '') AS "bio!: String",
                  COALESCE(a.youtube_video_url, '') AS "youtube_video_url!: String",
                  a.user_id IS NOT NULL AS "has_application!: bool",
                  EXISTS (SELECT 1 FROM ambassador_directory_listing l
                           WHERE l.user_id = u.user_id) AS "listed!: bool"
           FROM users u
           LEFT JOIN ambassador_applications a ON a.user_id = u.user_id
           WHERE u.user_id = ?"#,
        user_id
    )
    .fetch_optional(db())
    .await?
    else {
        return Ok(None);
    };

    let privileged = q.viewer_is_staff || q.viewer == Some(q.user_id);
    if !(row.listed || privileged && row.has_application) {
        return Ok(None);
    }

    let connected_event_ids = sqlx::query_scalar!(
        r#"SELECT entity_id FROM ambassador_connections WHERE user_id = ?"#,
        user_id
    )
    .fetch_all(db())
    .await?
    .into_iter()
    .filter_map(|id| id.as_ulid().ok())
    .collect();

    // orrery's "others": listed ambassadors in the same state.
    let nearby = if row.state.trim().is_empty() {
        Vec::new()
    } else {
        sqlx::query_as!(
            DirectoryRow,
            r#"SELECT l.user_id AS "user_id!", u.first_name AS "first_name!",
                      u.last_name AS "last_name!", u.image AS "image",
                      l.city AS "city!", l.state AS "state!"
               FROM ambassador_directory_listing l
               JOIN users u ON u.user_id = l.user_id
               WHERE l.user_id != ? AND lower(l.state) = lower(?)
               ORDER BY l.position IS NULL, l.position, l.user_id
               LIMIT ?"#,
            user_id,
            row.state,
            NEARBY_LIMIT,
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(Into::into)
        .collect()
    };

    Ok(Some(AmbassadorProfile {
        user_id: q.user_id,
        name: format!("{} {}", row.first_name, row.last_name)
            .trim()
            .to_string(),
        first_name: row.first_name.trim().to_string(),
        image: row.image,
        city: row.city,
        state: row.state,
        bio: row.bio,
        youtube_video_url: row.youtube_video_url,
        listed: row.listed,
        connected_event_ids,
        nearby,
    }))
}

/// Replace the mirror of orrery's ambassador connections with `pairs` (user
/// ULID, entity ULID) in one transaction. The feed is a snapshot rather than a
/// change log, since orrery keeps no dates on the relationship, so replacing is
/// how a removed connection goes away. A pair naming an account that has not
/// synced yet is skipped; the next snapshot brings it.
pub async fn replace_connections(pairs: &[(String, String)]) -> Result<(), Error> {
    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;
    sqlx::query!("DELETE FROM ambassador_connections")
        .execute(&mut tx)
        .await?;
    for (user_id, entity_id) in pairs {
        sqlx::query!(
            r#"INSERT OR IGNORE INTO ambassador_connections (user_id, entity_id)
               SELECT ?, ? WHERE EXISTS (SELECT 1 FROM users WHERE user_id = ?)"#,
            user_id,
            entity_id,
            user_id,
        )
        .execute(&mut tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Push the staff grid's filters onto a query already ending in a WHERE
/// clause. Shared by the paged list, its count, and the CSV export so the
/// three can never disagree about what "matching" means.
fn push_filters(qb: &mut QueryBuilder<Sqlite>, f: &AmbassadorFilters) {
    fn tristate(qb: &mut QueryBuilder<Sqlite>, column: &str, val: TriState) {
        if let Some(b) = val.as_bool() {
            qb.push(" AND a.\"");
            qb.push(column);
            qb.push("\" = ");
            qb.push_bind(if b { 1_i64 } else { 0_i64 });
        }
    }

    fn presence(qb: &mut QueryBuilder<Sqlite>, column: &str, val: DatePresence) {
        // A cleared date reaches us as NULL from the orrery sync but as an
        // empty string from the review form, so both spellings count as absent.
        match val {
            DatePresence::Any => {}
            DatePresence::Present => {
                qb.push(" AND a.\"");
                qb.push(column);
                qb.push("\" IS NOT NULL AND a.\"");
                qb.push(column);
                qb.push("\" != ''");
            }
            DatePresence::Absent => {
                qb.push(" AND (a.\"");
                qb.push(column);
                qb.push("\" IS NULL OR a.\"");
                qb.push(column);
                qb.push("\" = '')");
            }
        }
    }

    fn contains(qb: &mut QueryBuilder<Sqlite>, column: &str, needle: &str) {
        let trimmed = needle.trim();
        if trimmed.is_empty() {
            return;
        }
        qb.push(" AND a.\"");
        qb.push(column);
        qb.push("\" LIKE ");
        qb.push_bind(format!("%{}%", trimmed));
    }

    fn date_from(qb: &mut QueryBuilder<Sqlite>, column: &str, value: &str) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return;
        }
        qb.push(" AND a.\"");
        qb.push(column);
        qb.push("\" >= ");
        qb.push_bind(trimmed.to_string());
    }

    fn date_to(qb: &mut QueryBuilder<Sqlite>, column: &str, value: &str) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return;
        }
        // Dates are ISO text, and `submitted` carries a time, so an inclusive
        // end date compares against the end of that day.
        qb.push(" AND a.\"");
        qb.push(column);
        qb.push("\" <= ");
        qb.push_bind(format!("{trimmed}T23:59:59.999999Z"));
    }

    fn int_in(qb: &mut QueryBuilder<Sqlite>, column: &str, values: &[i64]) {
        if values.is_empty() {
            return;
        }
        qb.push(" AND a.\"");
        qb.push(column);
        qb.push("\" IN (");
        let mut first = true;
        for v in values {
            if !first {
                qb.push(", ");
            }
            first = false;
            qb.push_bind(*v);
        }
        qb.push(")");
    }

    if let Some(user_id) = f.user_id {
        qb.push(" AND a.\"user_id\" = ");
        qb.push_bind(user_id.to_string());
    }

    tristate(qb, "archived", f.archived);
    tristate(qb, "draft", f.draft);
    tristate(qb, "hidden", f.hidden);
    tristate(qb, "marked", f.marked);
    tristate(qb, "welcome_sent", f.welcome_sent);
    tristate(qb, "rejection_sent", f.rejection_sent);
    tristate(qb, "survey", f.survey);
    tristate(qb, "citscimonth", f.citscimonth);
    tristate(qb, "background_check", f.background_check);
    tristate(qb, "conduct", f.conduct);

    // Directory membership lives on the user record, not the application.
    match f.in_directory.as_bool() {
        Some(true) => qb.push(" AND u.\"ambassador\" IS NOT NULL"),
        Some(false) => qb.push(" AND u.\"ambassador\" IS NULL"),
        None => qb,
    };

    int_in(qb, "cohort", &f.cohort);
    int_in(qb, "app_status", &f.app_status);
    int_in(qb, "screening_status", &f.screening_status);

    presence(qb, "verified", f.verified_presence);
    presence(qb, "orientation", f.orientation_presence);
    presence(qb, "kit_sent", f.kit_sent_presence);
    presence(qb, "renewal_date", f.renewal_date_presence);

    date_from(qb, "submitted", &f.submitted_from);
    date_to(qb, "submitted", &f.submitted_to);
    date_from(qb, "verified", &f.verified_from);
    date_to(qb, "verified", &f.verified_to);
    date_from(qb, "renewal_date", &f.renewal_date_from);
    date_to(qb, "renewal_date", &f.renewal_date_to);

    contains(qb, "city", &f.city);
    contains(qb, "state", &f.state);
    contains(qb, "phone", &f.phone);
    contains(qb, "tee_size", &f.tee_size);
    contains(qb, "gender", &f.gender);
    contains(qb, "ethnicity", &f.ethnicity);
    contains(qb, "handles", &f.handles);
    contains(qb, "reviewers", &f.reviewers);
    contains(qb, "notes", &f.notes);

    let search = f.search.trim();
    if !search.is_empty() {
        let pattern = format!("%{}%", search);
        qb.push(" AND (u.\"first_name\" LIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" OR u.\"last_name\" LIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" OR (u.\"first_name\" || ' ' || u.\"last_name\") LIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" OR u.\"handle\" LIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" OR a.\"contact_email\" LIKE ");
        qb.push_bind(pattern);
        qb.push(")");
    }
}

const GRID_COLUMNS: &str = r#"
    a."user_id" AS "user_id",
    u."first_name" AS "first_name",
    u."last_name" AS "last_name",
    u."handle" AS "handle",
    u."ambassador" AS "ambassador",
    a."contact_email" AS "contact_email",
    a."city" AS "city",
    a."state" AS "state",
    a."phone" AS "phone",
    a."birthdate" AS "birthdate",
    a."tee_size" AS "tee_size",
    a."gender" AS "gender",
    a."ethnicity" AS "ethnicity",
    a."marked" AS "marked",
    a."archived" AS "archived",
    a."hidden" AS "hidden",
    a."draft" AS "draft",
    a."cohort" AS "cohort",
    a."app_status" AS "app_status",
    a."screening_status" AS "screening_status",
    a."verified" AS "verified",
    a."rejection_sent" AS "rejection_sent",
    a."welcome_sent" AS "welcome_sent",
    a."orientation" AS "orientation",
    a."kit_sent" AS "kit_sent",
    a."survey" AS "survey",
    a."handles" AS "handles",
    a."reviewers" AS "reviewers",
    a."notes" AS "notes",
    a."submitted" AS "submitted",
    a."updated_at" AS "updated_at"
"#;

const FROM_WHERE: &str = r#" FROM "ambassador_applications" a
    JOIN "users" u ON u."user_id" = a."user_id"
    WHERE 1 = 1"#;

fn grid_row(row: &sqlx::sqlite::SqliteRow) -> AmbassadorApplicationRow {
    let first_name: String = row.get("first_name");
    let last_name: String = row.get("last_name");
    let ambassador: Option<String> = row.get("ambassador");

    AmbassadorApplicationRow {
        user_id: row
            .get::<String, _>("user_id")
            .as_ulid()
            .unwrap_or_default(),
        name: format!("{} {}", first_name, last_name).trim().to_string(),
        handle: row.get("handle"),
        email: row.get("contact_email"),
        city: row.get("city"),
        state: row.get("state"),
        phone: row.get("phone"),
        birthdate: row.get("birthdate"),
        tee_size: row.get("tee_size"),
        gender: row.get("gender"),
        ethnicity: row.get("ethnicity"),
        in_directory: ambassador.is_some(),
        submitted: row.get("submitted"),
        updated_at: row.get("updated_at"),
        marked: row.get::<i64, _>("marked") != 0,
        archived: row.get::<i64, _>("archived") != 0,
        hidden: row.get::<i64, _>("hidden") != 0,
        draft: row.get::<i64, _>("draft") != 0,
        cohort: row.get("cohort"),
        app_status: AmbassadorAppStatus::from_i64(row.get("app_status")),
        screening_status: AmbassadorScreeningStatus::from_i64(row.get("screening_status")),
        verified: row.get("verified"),
        rejection_sent: row.get::<i64, _>("rejection_sent") != 0,
        welcome_sent: row.get::<i64, _>("welcome_sent") != 0,
        orientation: row.get("orientation"),
        kit_sent: row.get("kit_sent"),
        survey: row.get::<i64, _>("survey") != 0,
        handles: row.get("handles"),
        reviewers: row.get("reviewers"),
        notes: row.get("notes"),
    }
}

/// One page of ambassador applications for the staff processing grid
/// (`user.ambassador.list`), plus the total number of matches so the page can
/// render its pagination. Joined with the user record for identity/handle and
/// the derived directory state (`users.ambassador`).
pub async fn list(req: &Request) -> Result<AmbassadorListResponse, Error> {
    // Tolerant of an empty payload so a frontend deployed ahead of this
    // service still gets an unfiltered first page rather than an error.
    let request: AmbassadorListRequest =
        decode_structured_message(req.message.clone()).unwrap_or_default();

    let mut count_qb: QueryBuilder<Sqlite> =
        QueryBuilder::new(format!("SELECT COUNT(*){FROM_WHERE}"));
    push_filters(&mut count_qb, &request.filters);
    let total: i64 = count_qb.build_query_scalar().fetch_one(db()).await?;

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new("SELECT ");
    qb.push(GRID_COLUMNS);
    qb.push(FROM_WHERE);
    push_filters(&mut qb, &request.filters);
    qb.push(" ORDER BY a.\"submitted\" DESC");

    if request.per_page > 0 {
        let page = request.page.max(1);
        qb.push(" LIMIT ");
        qb.push_bind(request.per_page);
        qb.push(" OFFSET ");
        qb.push_bind((page - 1) * request.per_page);
    }

    let rows = qb.build().fetch_all(db()).await?;

    Ok(AmbassadorListResponse {
        rows: rows.iter().map(grid_row).collect(),
        total,
    })
}

/// Every application matching the grid's filters, with the applicant's
/// long-form answers, for the staff CSV export (`user.ambassador.export`).
/// Unpaged by design — the caller asked for the whole result set.
pub async fn export(req: &Request) -> Result<Vec<AmbassadorExportRow>, Error> {
    let request: AmbassadorListRequest =
        decode_structured_message(req.message.clone()).unwrap_or_default();

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"SELECT
             a."user_id" AS "user_id",
             u."username" AS "username",
             u."first_name" AS "first_name",
             u."last_name" AS "last_name",
             u."handle" AS "handle",
             u."ambassador" AS "ambassador",
             a."contact_email" AS "contact_email",
             a."submitted" AS "submitted",
             a."updated_at" AS "updated_at",
             a."draft" AS "draft",
             a."hidden" AS "hidden",
             a."marked" AS "marked",
             a."archived" AS "archived",
             a."cohort" AS "cohort",
             a."app_status" AS "app_status",
             a."screening_status" AS "screening_status",
             a."verified" AS "verified",
             a."orientation" AS "orientation",
             a."kit_sent" AS "kit_sent",
             a."renewal_date" AS "renewal_date",
             a."birthdate" AS "birthdate",
             a."welcome_sent" AS "welcome_sent",
             a."rejection_sent" AS "rejection_sent",
             a."survey" AS "survey",
             a."citscimonth" AS "citscimonth",
             a."background_check" AS "background_check",
             a."conduct" AS "conduct",
             a."phone" AS "phone",
             a."city" AS "city",
             a."state" AS "state",
             a."tee_size" AS "tee_size",
             a."gender" AS "gender",
             a."ethnicity" AS "ethnicity",
             a."self_description" AS "self_description",
             a."handles" AS "handles",
             a."introduction" AS "introduction",
             a."introduction_other" AS "introduction_other",
             a."experience" AS "experience",
             a."motivation" AS "motivation",
             a."related" AS "related",
             a."vision" AS "vision",
             a."activities" AS "activities",
             a."community_access" AS "community_access",
             a."signature_name" AS "signature_name",
             a."signature_date" AS "signature_date",
             a."questions" AS "questions",
             a."bio" AS "bio",
             a."notes" AS "notes",
             a."reviewers" AS "reviewers",
             a."youtube_video_url" AS "youtube_video_url""#,
    );
    qb.push(FROM_WHERE);
    push_filters(&mut qb, &request.filters);
    qb.push(" ORDER BY a.\"submitted\" DESC");

    let rows = qb.build().fetch_all(db()).await?;

    Ok(rows
        .iter()
        .map(|row| {
            let ambassador: Option<String> = row.get("ambassador");
            AmbassadorExportRow {
                user_ulid: row.get("user_id"),
                username: row.get("username"),
                first_name: row.get("first_name"),
                last_name: row.get("last_name"),
                handle: row.get("handle"),
                email: row.get("contact_email"),
                in_directory: ambassador.is_some(),
                submitted: row.get("submitted"),
                updated_at: row.get("updated_at"),
                draft: row.get::<i64, _>("draft") != 0,
                hidden: row.get::<i64, _>("hidden") != 0,
                marked: row.get::<i64, _>("marked") != 0,
                archived: row.get::<i64, _>("archived") != 0,
                cohort: row.get("cohort"),
                app_status: AmbassadorAppStatus::from_i64(row.get("app_status"))
                    .label()
                    .to_string(),
                screening_status: AmbassadorScreeningStatus::from_i64(row.get("screening_status"))
                    .label()
                    .to_string(),
                verified: row.get("verified"),
                orientation: row.get("orientation"),
                kit_sent: row.get("kit_sent"),
                renewal_date: row.get("renewal_date"),
                birthdate: row.get("birthdate"),
                welcome_sent: row.get::<i64, _>("welcome_sent") != 0,
                rejection_sent: row.get::<i64, _>("rejection_sent") != 0,
                survey: row.get::<i64, _>("survey") != 0,
                citscimonth: row.get::<i64, _>("citscimonth") != 0,
                background_check: row.get::<i64, _>("background_check") != 0,
                conduct: row.get::<i64, _>("conduct") != 0,
                phone: row.get("phone"),
                city: row.get("city"),
                state: row.get("state"),
                tee_size: row.get("tee_size"),
                gender: row.get("gender"),
                ethnicity: row.get("ethnicity"),
                self_description: row.get("self_description"),
                handles: row.get("handles"),
                introduction: row.get("introduction"),
                introduction_other: row.get("introduction_other"),
                experience: row.get("experience"),
                motivation: row.get("motivation"),
                related: row.get("related"),
                vision: row.get("vision"),
                activities: row.get("activities"),
                community_access: row.get("community_access"),
                signature_name: row.get("signature_name"),
                signature_date: row.get("signature_date"),
                questions: row.get("questions"),
                bio: row.get("bio"),
                notes: row.get("notes"),
                reviewers: row.get("reviewers"),
                youtube_video_url: row.get("youtube_video_url"),
            }
        })
        .collect())
}

/// Apply a staff review edit (`user.ambassador.review`) and drive the native
/// ambassador state machine. Persists the editable fields, then reconciles the
/// Certified Ambassador badge and public-directory membership.
pub async fn review(req: &Request) -> Result<(), Error> {
    let update: AmbassadorReviewUpdate = decode_structured_message(req.message.clone())?;
    let user_id_str = update.user_id.to_string();

    // What the application said before this edit, to tell which of the
    // review's events it is the moment of.
    let before = sqlx::query!(
        r#"SELECT "orientation", "app_status" FROM "ambassador_applications" WHERE "user_id" = ?"#,
        user_id_str
    )
    .fetch_optional(db())
    .await?;

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

    // Events that send nothing of their own, but cancel the reminder an
    // accepted Ambassador gets while their onboarding is unfinished.
    let had_orientation = before.as_ref().is_some_and(|row| row.orientation.is_some());

    if update.orientation.is_some() && !had_orientation {
        trigger_email(
            TriggerEvent::AmbassadorOrientationRecorded,
            update.user_id,
            None,
        )
        .await?;
    }

    let was_champion = before.as_ref().is_some_and(|row| {
        AmbassadorAppStatus::from_i64(row.app_status) == AmbassadorAppStatus::Champion
    });

    if update.app_status == AmbassadorAppStatus::Champion && !was_champion {
        trigger_email(TriggerEvent::AmbassadorDemoted, update.user_id, None).await?;
    }

    Ok(())
}

/// Reconcile the Certified Ambassador badge and public-directory membership
/// from a review edit. Mirrors orrery's `ambassador_application_pre_save`
/// precedence (Champion → demote > Rejected > screening Failed/International >
/// verified) and is idempotent, so it converges with the orrery state sync
/// ([`crate::apply_ambassador_state`]).
///
/// Two distinct outcomes, matching orrery:
/// - **badge** (Certified Ambassador) is earned once a `verified` date is set
///   and no earlier branch of the precedence has fired (orrery's
///   `verified_enter`);
/// - **directory** membership (orrery's `passed`) additionally requires the
///   `Accepted` verdict and a `Verified` background screening, and is recorded
///   as a non-null `users.ambassador` timestamp.
///
/// Demoting to `Champion` reverts both (orrery's `revert_to_champion`).
///
/// The stage this leaves the person at is not recorded anywhere: it is derived
/// from these outcomes by [`AmbassadorStage::derive`].
/// The tags granted along the path past Champion, in the order a user walks it.
///
/// Orrery's `POST_CHAMPION_TAGS`, and they must stay in step with it: they are
/// cumulative, so somebody in the directory carries all three, and a demotion
/// to Champion discards all three (orrery's `revert_to_champion`). Stages off
/// that path -- rejected, failed, international -- neither grant nor discard.
///
/// Kept written here rather than left to the orrery sync because the Constant
/// Contact mirror treats these names as ours to manage: a contact holding one
/// that this service does not know about would have it stripped on the first
/// push after the sweep changes hands.
pub const PATH_PAST_CHAMPION: &[&str] = &[
    "scistarter-ambassador-application-submitted",
    "scistarter-ambassador-verified",
    "scistarter-ambassador",
];

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
    // orrery grants the badge in `verified_enter`, which a `verified` date alone
    // reaches once the earlier branches of its precedence have not fired: the
    // verdict on the application is not consulted. A staff member who dates the
    // verification has accepted the applicant, whatever the verdict field still
    // says.
    let badge_earned = !demote && !blocked && verified;

    // The directory is orrery's `passed`, which only `promote_to_ambassador`
    // reaches — a verdict, not the verification date. `Ambassador` is that
    // verdict; `Accepted` is kept beside it because hydra used to stand in for
    // the missing variant with it, and the applications staff accepted without
    // ever promoting are in the directory on its strength.
    let promoted = matches!(
        app_status,
        AmbassadorAppStatus::Accepted | AmbassadorAppStatus::Ambassador
    );
    let in_directory = badge_earned && promoted && screening == AmbassadorScreeningStatus::Verified;

    // Date the way the route closed, if this review closed it, and fire the
    // matching entry email on that write alone. Which verdicts close the route
    // is asked of the same place the funnel asks it, so that what gets dated
    // here and what gets reported there cannot come apart. A demotion to
    // Champion is the one verdict that reaches back past a closure, so it
    // records nothing. The `verified`/`passed` (accepted) email is the
    // Certified Ambassador badge notification, handled below by the `had_badge`
    // guard; `failed` has no email, matching orrery.
    let closed = (!demote)
        .then(|| AmbassadorStage::closed_by(app_status, screening))
        .flatten()
        .and_then(ClosedRoute::of);

    if let Some(closed) = closed
        && close_route(&user_id_str, closed, &now).await?
    {
        match closed {
            ClosedRoute::Rejected => {
                trigger_email(TriggerEvent::AmbassadorRejected, user_id, None).await?;
            }
            ClosedRoute::International => {
                trigger_email(TriggerEvent::AmbassadorInternational, user_id, None).await?;
            }
            ClosedRoute::ScreeningFailed => {}
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

        for tag in PATH_PAST_CHAMPION {
            send("user.tags.remove", (user_id_str.clone(), tag.to_string())).await?;
        }
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

        // Orrery grants the verified tag in `verified_enter`, alongside the
        // same badge. The application tag goes back on here too: the path is
        // cumulative, and anybody who earned the badge applied for it, so
        // re-adding it is what makes a re-promotion after a demotion end up
        // where orrery's `restore_from_champion` would leave it. Both writes
        // are idempotent.
        for tag in [
            "scistarter-ambassador-application-submitted",
            "scistarter-ambassador-verified",
        ] {
            send("user.tags.add", (user_id_str.clone(), tag.to_string())).await?;
        }

        if !had_badge {
            // Native staff action (not a sync), so an email notification is
            // appropriate here.
            trigger_email(
                TriggerEvent::BadgeAmbassador,
                user_id,
                Some(&crate::workflow::badge_email_fields(
                    Badge::SciStarterAmbassador,
                    &now,
                )),
            )
            .await?;
        }
    }

    // Directory membership: set when passed, cleared otherwise (including on
    // demote). Recorded on the user record so it drives the public directory,
    // the map, and the profile "Ambassador" chip.
    if in_directory {
        let joined = sqlx::query!(
            "UPDATE users SET ambassador = ? WHERE user_id = ? AND ambassador IS NULL",
            now,
            user_id_str
        )
        .execute(db())
        .await?
        .rows_affected()
            > 0;

        if joined {
            trigger_email(TriggerEvent::AmbassadorPromoted, user_id, None).await?;
        }

        // Orrery grants this in `passed_enter`, beside the same timestamp.
        send(
            "user.tags.add",
            (user_id_str.clone(), "scistarter-ambassador".to_string()),
        )
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

    // Only listed ambassadors are contactable: the form is on the public
    // profile, which an unlisted one does not have.
    let is_ambassador = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM ambassador_directory_listing WHERE user_id = ?",
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
    .map(|r| {
        format!("{} {}", r.first_name, r.last_name)
            .trim()
            .to_string()
    })
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

/// Everyone currently at one of the requested funnel stages
/// (`user.ambassador.stages`), newest arrival first.
///
/// Backs the staff funnel exports. The stage is derived per person by
/// [`AmbassadorStage::derive`] rather than read from a column, so it cannot
/// disagree with the milestones it is made of; the "since" date is the date of
/// the milestone that decided the stage. Test accounts are left out; staff
/// accounts are not, because a staff member who took the training is a real
/// person who took the training — the same rule orrery applies.
pub async fn stages(req: &Request) -> Result<Vec<AmbassadorStageRow>, Error> {
    let request: AmbassadorStagesRequest =
        decode_structured_message(req.message.clone()).unwrap_or_default();

    if request.stages.is_empty() {
        return Ok(Vec::new());
    }

    // Everybody who has reached any milestone at all. Those who have reached
    // none are at no stage, so they cannot match whatever was asked for.
    let rows = sqlx::query!(
        r#"SELECT u."first_name" AS "first_name!",
                  u."last_name" AS "last_name!",
                  u."handle" AS "handle!",
                  u."primary_email" AS "primary_email!",
                  u."ambassador" AS "ambassador?",
                  p."tutorial_completed_at" AS "tutorial_completed_at?",
                  p."rejected_at" AS "rejected_at?",
                  p."international_at" AS "international_at?",
                  p."screening_failed_at" AS "screening_failed_at?",
                  a."app_status" AS "app_status?",
                  a."screening_status" AS "screening_status?",
                  a."verified" AS "verified?",
                  a."submitted" AS "submitted?",
                  a."draft" AS "draft?"
             FROM "ambassador_progress" p
             JOIN "users" u ON u."user_id" = p."user_id"
             LEFT JOIN "ambassador_applications" a ON a."user_id" = p."user_id"
            WHERE u."is_test" = 0
              AND (p."tutorial_completed_at" IS NOT NULL
                   OR p."rejected_at" IS NOT NULL
                   OR p."international_at" IS NOT NULL
                   OR p."screening_failed_at" IS NOT NULL
                   OR u."ambassador" IS NOT NULL
                   OR a."user_id" IS NOT NULL)"#
    )
    .fetch_all(db())
    .await?;

    let mut stages: Vec<AmbassadorStageRow> = rows
        .into_iter()
        .filter_map(|row| {
            let submitted = row.submitted.filter(|_| row.draft == Some(0));

            let milestones = AmbassadorMilestones {
                tutorial_completed: row.tutorial_completed_at.is_some(),
                application_submitted: submitted.is_some(),
                verified: row.verified.is_some(),
                in_directory: row.ambassador.is_some(),
                app_status: AmbassadorAppStatus::from_i64(row.app_status.unwrap_or_default()),
                screening_status: AmbassadorScreeningStatus::from_i64(
                    row.screening_status.unwrap_or_default(),
                ),
            };

            let stage = AmbassadorStage::derive(&milestones)?;

            if !request.stages.contains(&stage) {
                return None;
            }

            // The date of the milestone that decided the stage, so that the
            // export reports when the person arrived where they are rather
            // than when their record was last touched.
            let since = match stage {
                AmbassadorStage::Tutorial => row.tutorial_completed_at,
                AmbassadorStage::Application => submitted,
                AmbassadorStage::Verified => row.verified,
                AmbassadorStage::Passed => row.ambassador,
                AmbassadorStage::Rejected => row.rejected_at,
                AmbassadorStage::International => row.international_at,
                AmbassadorStage::Failed => row.screening_failed_at,
            }
            .unwrap_or_default();

            Some(AmbassadorStageRow {
                first_name: row.first_name,
                last_name: row.last_name,
                handle: row.handle,
                email: row.primary_email,
                stage,
                // Stored as a full timestamp; the export wants the date.
                since: since.get(0..10).unwrap_or_default().to_string(),
            })
        })
        .collect();

    stages.sort_by(|a, b| b.since.cmp(&a.since).then_with(|| b.email.cmp(&a.email)));

    Ok(stages)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The directory view names the Champion verdict by its number.
    ///
    /// `ambassador_directory_members` excludes `app_status = 5` to keep a
    /// demoted Ambassador out of the public directory, and SQL cannot import
    /// the enum to say so. Renumbering the variant without editing the view
    /// would quietly re-list everybody on a hiatus, so the number is pinned
    /// here instead.
    #[test]
    fn champion_is_the_verdict_the_directory_view_excludes() {
        assert_eq!(AmbassadorAppStatus::Champion.as_i64(), 5);
    }

    #[test]
    fn near_reads_city_and_state_state_code_or_anything() {
        assert_eq!(NearFilter::parse("  "), NearFilter::Anywhere);
        assert_eq!(
            NearFilter::parse("Sioux Falls, SD"),
            NearFilter::CityAndState {
                city: "Sioux Falls".into(),
                state: "SD".into()
            }
        );
        assert_eq!(NearFilter::parse("SD"), NearFilter::State("SD".into()));
        assert_eq!(
            NearFilter::parse("NEW YORK"),
            NearFilter::State("NEW YORK".into())
        );
        assert_eq!(
            NearFilter::parse("sioux"),
            NearFilter::Loose("sioux".into())
        );
        // A ZIP code has no letters, so it is not "in capitals".
        assert_eq!(
            NearFilter::parse("57104"),
            NearFilter::Loose("57104".into())
        );
    }

    #[test]
    fn contains_pattern_takes_wildcards_literally() {
        assert_eq!(contains_pattern("Doe"), "%Doe%");
        assert_eq!(contains_pattern("50%_\\"), "%50\\%\\_\\\\%");
    }
}
