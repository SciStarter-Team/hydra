use anyhow::Error;
use async_nats::service::Request;
use common::{
    badge::Badge,
    message::user::{
        AdminPasswordResetRequest, AdminPasswordResetResponse, BadgeHolderInfo,
        BadgeHoldersRequest, BadgeHoldersResponse, BasicInfo, InterestedRequest, InterestedUser,
        MasqueradeRequest, PRIVATE_DISPLAY_NAME, PageManagerSyncRequest, PageManagerSyncResponse,
        PageManagerUpdateRequest, PageManagerUpdateResponse, TagCohortMember, TagCohortRequest,
        UserAndBadge, UserBadge, UserDetails, UserDetailsBatchGetRequest,
        UserDetailsBatchGetResponse, UserDetailsCreateRequest, UserDetailsDeleteRequest,
        UserDetailsDeleteResponse, UserDetailsExportRequest, UserDetailsExportResponse,
        UserDetailsGetRequest, UserDetailsListRequest, UserDetailsListResponse,
        UserDetailsResponse, UserDetailsSearchRequest, UserDetailsUpdateRequest, UserDisplayInfo,
        UserDisplayInfoBatchRequest, UserDisplayInfoBatchResponse, UserEmail, UserEmailAddRequest,
        UserEmailDeleteRequest, UserEmailInfo, UserEmailInfoRequest, UserEmailResponse,
        UserEmailToggleVerifiedRequest, UserEmailUpdateRequest, UserNameMapping, UserPointsRequest,
        UserPointsResponse,
    },
    service::{decode_structured_message, log_activity, send},
    shared::{db, hash_identifier, hash_identifier_capfirst, hash_identifier_lower},
    timestring::TimeString,
    transaction::Immediate,
    ulid::AsUlid,
};
use ulid::Ulid;

#[derive(Debug)]
struct BasicInfoDb {
    user_id: String,
    handle: String,
    is_staff: bool,
    is_superuser: bool,
    public: bool,
    protected: bool,
    point: Option<String>,
}

impl TryFrom<BasicInfoDb> for BasicInfo {
    type Error = anyhow::Error;

    fn try_from(db_info: BasicInfoDb) -> Result<Self, Self::Error> {
        Ok(BasicInfo {
            user_id: db_info.user_id.as_ulid()?,
            handle: db_info.handle,
            is_staff: db_info.is_staff,
            is_superuser: db_info.is_superuser,
            public: db_info.public,
            protected: db_info.protected,
            needs_point: db_info.point.as_ref().map_or(true, |p| p.trim().is_empty()),
        })
    }
}

/// Calculate user's verified status based on their email records
async fn calculate_user_verified_status(user_id: Ulid) -> Result<bool, Error> {
    let user_id_str = user_id.to_string();
    let verified_count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) > 0 FROM "emails" WHERE "user_id" = ? AND "verified" = 1"#,
        user_id_str
    )
    .fetch_one(db())
    .await?;

    Ok(verified_count != 0)
}

/// Update user's verified status in the database based on their email records
async fn update_user_verified_status(user_id: Ulid) -> Result<(), Error> {
    let calculated_verified = calculate_user_verified_status(user_id).await?;
    let user_id_str = user_id.to_string();

    sqlx::query!(
        r#"UPDATE "users" SET "verified" = ? WHERE "user_id" = ?"#,
        calculated_verified,
        user_id_str
    )
    .execute(db())
    .await?;

    Ok(())
}

pub async fn basic_info_by_username(req: &Request) -> Result<Option<BasicInfo>, Error> {
    let email: String = flexbuffers::from_slice(&req.message.payload)?;

    let result = sqlx::query_as!(
        BasicInfoDb,
        r#"select
          u."user_id",
          u."handle",
          u."is_staff" as "is_staff: bool",
          u."is_superuser" as "is_superuser: bool",
          u."public" as "public: bool",
          u."protected" as "protected: bool",
          AsText(u."point") as "point: String"
        from "users" u
        join "emails" e on u."user_id" = e."user_id"
        where e."email" = ?"#,
        email,
    )
    .fetch_optional(db())
    .await?;

    match result {
        Some(db_info) => Ok(Some(db_info.try_into()?)),
        None => Ok(None),
    }
}

pub async fn basic_info_by_user_id(req: &Request) -> Result<BasicInfo, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;

    // Return Err on not-found rather than Ok(None). All callers in the
    // workspace decode the response as BasicInfo; with Option<BasicInfo>
    // the `None` case wire-encodes as a unit value that the caller-side
    // serde rejects with "invalid type: unit value, expected struct
    // BasicInfo" — a confusing error that propagates through wrapping
    // services as a 500. An explicit Err lets `.await?` callers fail
    // cleanly and `.await.ok()` callers degrade to None.
    let result = sqlx::query_as!(
        BasicInfoDb,
        r#"select
          "user_id",
          "handle",
          "is_staff" as "is_staff: bool",
          "is_superuser" as "is_superuser: bool",
          "public" as "public: bool",
          "protected" as "protected: bool",
          AsText("point") as "point: String"
        from "users"
        where "user_id" = ?"#,
        user_id,
    )
    .fetch_optional(db())
    .await?
    .ok_or_else(|| anyhow::anyhow!("User not found"))?;

    Ok(result.try_into()?)
}

pub async fn search_by_handle(req: &Request) -> Result<Vec<BasicInfo>, Error> {
    let search_term: String = flexbuffers::from_slice(&req.message.payload)?;

    // Cap results so a short prefix can't be used to bulk-extract the
    // public user base by iterating handles.
    let results = sqlx::query_as!(
        BasicInfoDb,
        r#"select
          "user_id",
          "handle",
          "is_staff" as "is_staff: bool",
          "is_superuser" as "is_superuser: bool",
          "public" as "public: bool",
          "protected" as "protected: bool",
          AsText("point") as "point: String"
        from "users"
        where "handle" LIKE ? || '%'
          AND "public" = 1
          AND "protected" = 0
        order by "handle"
        limit 20"#,
        search_term,
    )
    .fetch_all(db())
    .await?;

    let mut users = Vec::new();
    for db_info in results {
        users.push(db_info.try_into()?);
    }

    Ok(users)
}

pub async fn user_by_email(
    req: &Request,
) -> Result<common::message::user::UserByEmailResponse, Error> {
    use common::message::user::{UserByEmailRequest, UserByEmailResponse};

    let request: UserByEmailRequest = decode_structured_message(req.message.clone())?;

    // Look up user by primary email
    let result = sqlx::query!(
        r#"SELECT user_id, handle, first_name, last_name, primary_email
        FROM users
        WHERE primary_email = ?"#,
        request.email
    )
    .fetch_optional(db())
    .await?;

    if let Some(row) = result {
        Ok(UserByEmailResponse {
            found: true,
            user_id: Some(row.user_id),
            handle: Some(row.handle),
            first_name: Some(row.first_name),
            last_name: Some(row.last_name),
            email: Some(row.primary_email),
        })
    } else {
        Ok(UserByEmailResponse {
            found: false,
            user_id: None,
            handle: None,
            first_name: None,
            last_name: None,
            email: None,
        })
    }
}

/// Get all tags for a user
pub async fn get_user_tags(req: &Request) -> Result<Vec<String>, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;

    let tags = sqlx::query_scalar!(
        r#"SELECT "tag" FROM "user_tags" WHERE "user_id" = ? ORDER BY "tag""#,
        user_id
    )
    .fetch_all(db())
    .await?;

    Ok(tags)
}

/// Everyone carrying `tag`, with every tag each of them holds
/// (`user.tags.cohort`).
///
/// Backs staff cohort reports, which ask "who is in this group, and what else
/// is true of them". Test accounts are excluded: they are in the tag tables for
/// the same reasons real users are, and would otherwise inflate every count.
pub async fn tag_cohort(req: &Request) -> Result<Vec<TagCohortMember>, Error> {
    let request: TagCohortRequest =
        decode_structured_message(req.message.clone()).unwrap_or_default();

    if request.tag.is_empty() {
        return Ok(Vec::new());
    }

    // One query for the cohort and one for their tags, rather than one tag
    // query per member: `group_concat` keeps it to two round trips without
    // materializing a row per (user, tag) pair for the whole table.
    let rows = sqlx::query!(
        r#"SELECT u."user_id" AS "user_id!",
                  u."primary_email" AS "primary_email!",
                  u."first_name" AS "first_name!",
                  u."last_name" AS "last_name!",
                  u."date_joined" AS "date_joined!",
                  (SELECT group_concat(t2."tag", char(10))
                     FROM "user_tags" t2
                    WHERE t2."user_id" = u."user_id") AS "tags?"
             FROM "user_tags" t
             JOIN "users" u ON u."user_id" = t."user_id"
            WHERE t."tag" = ? AND u."is_test" = 0
            ORDER BY u."date_joined""#,
        request.tag,
    )
    .fetch_all(db())
    .await?;

    let mut members = Vec::with_capacity(rows.len());

    for row in rows {
        members.push(TagCohortMember {
            user_id: row.user_id.as_ulid()?,
            email: row.primary_email,
            first_name: row.first_name,
            last_name: row.last_name,
            date_joined: row.date_joined,
            // Tags cannot contain a newline, so it is safe as a separator.
            tags: row
                .tags
                .unwrap_or_default()
                .split('\n')
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .collect(),
        });
    }

    Ok(members)
}

/// Hand a staff member the identity of the account owning `email`
/// (`user.admin.masquerade`), recording that it happened.
///
/// Returns `None` when no account has that address, so the caller can say so
/// rather than reporting a failure.
///
/// The audit row is written before the identity is returned, and by this call
/// rather than the caller's, so a masquerade cannot happen without a record of
/// it. Authorization is the caller's to enforce — the whole `/staff/` tree it
/// is reached from is behind the staff gate — but the record is not.
pub async fn masquerade(req: &Request) -> Result<Option<BasicInfo>, Error> {
    let request: MasqueradeRequest =
        decode_structured_message(req.message.clone()).unwrap_or_default();

    let result = sqlx::query_as!(
        BasicInfoDb,
        r#"select
          u."user_id",
          u."handle",
          u."is_staff" as "is_staff: bool",
          u."is_superuser" as "is_superuser: bool",
          u."public" as "public: bool",
          u."protected" as "protected: bool",
          AsText(u."point") as "point: String"
        from "users" u
        join "emails" e on u."user_id" = e."user_id"
        where e."email" = ?"#,
        request.email,
    )
    .fetch_optional(db())
    .await?;

    let Some(db_info) = result else {
        return Ok(None);
    };

    let masquerade_id = Ulid::new().to_string();
    let now = TimeString::now();

    sqlx::query!(
        r#"INSERT INTO "masquerades" ("masquerade_id", "when", "staff_user_id", "target_user_id")
           VALUES (?, ?, ?, ?)"#,
        masquerade_id,
        now,
        request.staff_user_id,
        db_info.user_id,
    )
    .execute(db())
    .await?;

    eprintln!(
        "Staff masquerade: {} is now acting as {}",
        request.staff_user_id, db_info.user_id
    );

    Ok(Some(db_info.try_into()?))
}

/// Everyone who has shown interest in any of a set of projects
/// (`user.interest.by_projects`).
///
/// Joins and bookmarks are read here; reviewers are supplied by the caller,
/// which asks the log service for them. The three sources are unioned so each
/// person appears once, with a flag per source saying how their interest
/// showed.
///
/// A reviewer whose account has since been deleted simply drops out, because
/// the join against `users` is what produces the row.
pub async fn interested_by_projects(req: &Request) -> Result<Vec<InterestedUser>, Error> {
    use sqlx::{QueryBuilder, Row as _, Sqlite};

    let request: InterestedRequest =
        decode_structured_message(req.message.clone()).unwrap_or_default();

    if request.project_ids.is_empty() && request.reviewer_ids.is_empty() {
        return Ok(Vec::new());
    }

    // One query, with the three sources as correlated existence tests. Doing
    // it as three queries and merging in Rust would need the same three index
    // lookups and then a merge that could disagree with itself about who is
    // who.
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"SELECT u."user_id" AS "user_id",
                  u."first_name" AS "first_name",
                  u."last_name" AS "last_name",
                  u."primary_email" AS "primary_email",
                  EXISTS (SELECT 1 FROM "project_joins" j
                           WHERE j."user_id" = u."user_id" AND j."project_id" IN ("#,
    );

    // An empty list is spelled as a bound NULL rather than an empty `IN ()`,
    // which SQLite will not parse.
    let push_ids = |qb: &mut QueryBuilder<Sqlite>, ids: &[String]| {
        if ids.is_empty() {
            qb.push("NULL");
        } else {
            let mut separated = qb.separated(", ");
            for id in ids {
                separated.push_bind(id.clone());
            }
        }
    };

    push_ids(&mut qb, &request.project_ids);
    qb.push(
        r#")) AS "joined",
                  EXISTS (SELECT 1 FROM "bookmarks" b
                           WHERE b."user_id" = u."user_id" AND b."project_id" IN ("#,
    );
    push_ids(&mut qb, &request.project_ids);
    // COALESCE, because an empty reviewer list is spelled `IN (NULL)`, whose
    // result is NULL rather than false — and a NULL here would both drop out
    // of the WHERE and fail to decode as a boolean.
    qb.push(r#")) AS "bookmarked", COALESCE(u."user_id" IN ("#);
    push_ids(&mut qb, &request.reviewer_ids);
    qb.push(
        r#"), 0) AS "reviewed"
             FROM "users" u
            WHERE "joined" OR "bookmarked" OR "reviewed"
            ORDER BY u."user_id""#,
    );

    let rows = qb.build().fetch_all(db()).await?;

    let mut users = Vec::with_capacity(rows.len());

    for row in rows {
        let user_id: String = row.get("user_id");

        users.push(InterestedUser {
            user_id: user_id.as_ulid()?,
            first_name: row.get("first_name"),
            last_name: row.get("last_name"),
            email: row.get("primary_email"),
            joined: row.get::<i64, _>("joined") != 0,
            bookmarked: row.get::<i64, _>("bookmarked") != 0,
            reviewed: row.get::<i64, _>("reviewed") != 0,
        });
    }

    Ok(users)
}

/// One page of user locations for the staff user map (`user.points.page`).
///
/// Only accounts that have a location appear, and only the geometry travels:
/// the map plots anonymous dots, so sending anything identifying with them
/// would be handing out more than the map draws.
pub async fn points_page(req: &Request) -> Result<UserPointsResponse, Error> {
    let request: UserPointsRequest =
        decode_structured_message(req.message.clone()).unwrap_or_default();

    let points = sqlx::query_scalar!(
        r#"SELECT AsGeoJSON("point") AS "point!: String"
             FROM "users"
            WHERE "point" IS NOT NULL AND "is_test" = 0
            ORDER BY "user_id"
            LIMIT ? OFFSET ?"#,
        request.limit,
        request.offset,
    )
    .fetch_all(db())
    .await?;

    Ok(UserPointsResponse { points })
}

/// Set user tags (replaces all existing tags)
pub async fn set_user_tags(req: &Request) -> Result<(), Error> {
    let (user_id, tags): (String, Vec<String>) = flexbuffers::from_slice(&req.message.payload)?;

    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

    // Remove all existing tags for this user
    sqlx::query!(r#"DELETE FROM "user_tags" WHERE "user_id" = ?"#, user_id)
        .execute(&mut tx)
        .await?;

    // Insert new tags
    for tag in &tags {
        sqlx::query!(
            r#"INSERT INTO "user_tags" ("user_id", "tag") VALUES (?, ?)"#,
            user_id,
            tag
        )
        .execute(&mut tx)
        .await?;
    }

    tx.commit().await?;

    // Constant Contact holds some of these tags directly, and derives the
    // "projects participated in" and "library affiliations" fields from others,
    // so any change here may have made their copy stale. Flagged unconditionally
    // rather than testing which tags matter: that test already exists, in the
    // digest the sweep compares before sending, and a second copy of it here
    // would be one more thing to keep in step. An irrelevant tag costs a
    // cleared flag and no request at all.
    crate::constant_contact::mark_dirty(&user_id).await?;

    // Notify log service of tag update
    send(
        "log.cache.user_tags_update",
        (user_id.clone(), tags.clone()),
    )
    .await?;

    Ok(())
}

/// Add a tag to a user
pub async fn add_user_tag(req: &Request) -> Result<(), Error> {
    let (user_id, tag): (String, String) = flexbuffers::from_slice(&req.message.payload)?;

    apply_tag(&user_id, &tag, TagSource::Site).await
}

/// Add a tag that arrived with the person rather than from something the site
/// did: the `?tag=` the login page stashes in the session, landing on an
/// account that already exists.
///
/// A separate endpoint rather than a flag on the existing one because where a
/// tag came from is not something the receiving end can work out, and it
/// decides whether Constant Contact is allowed to see a name that no allowlist
/// could have predicted.
pub async fn add_signup_tag(req: &Request) -> Result<(), Error> {
    let (user_id, tag): (String, String) = flexbuffers::from_slice(&req.message.payload)?;

    apply_tag(&user_id, &tag, TagSource::Signup).await
}

/// Where a tag being applied came from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TagSource {
    /// Something the site itself did: a badge earned, a project joined.
    Site,
    /// The link the person arrived on.
    Signup,
}

async fn apply_tag(user_id: &str, tag: &str, source: TagSource) -> Result<(), Error> {
    let tag = common::message::user::normalize_tag(tag);

    if tag.is_empty() {
        return Ok(());
    }

    // Insert the tag (ignore if already exists)
    sqlx::query!(
        r#"INSERT OR IGNORE INTO "user_tags" ("user_id", "tag") VALUES (?, ?)"#,
        user_id,
        tag
    )
    .execute(db())
    .await?;

    if source == TagSource::Signup {
        remember_signup_tag(user_id, &tag).await?;

        // A tag that credits its cohort with a training module credits it
        // here too. The account already exists, so it did not pass the rule
        // in `create_account`, and a partner hands out one link to a cohort
        // whose members mostly do already have accounts.
        //
        // Decided here rather than at each login seam for the reason the tag
        // itself is recorded here: this is the one place a signup-context tag
        // reaches an account that already exists, so every lane that gets a
        // person signed in -- password, passkey, whatever comes next -- obeys
        // it without having to remember to.
        if common::message::user::signup_grants_foundations(&[&tag]) {
            crate::workflow::credit_foundations_cohort(user_id).await?;
        }
    }

    // Constant Contact holds some of these tags directly, and derives the
    // "projects participated in" and "library affiliations" fields from others,
    // so any change here may have made their copy stale. Flagged unconditionally
    // rather than testing which tags matter: that test already exists, in the
    // digest the sweep compares before sending, and a second copy of it here
    // would be one more thing to keep in step. An irrelevant tag costs a
    // cleared flag and no request at all.
    crate::constant_contact::mark_dirty(user_id).await?;

    // Get all current tags and notify log service
    let tags = sqlx::query_scalar!(
        r#"SELECT "tag" FROM "user_tags" WHERE "user_id" = ? ORDER BY "tag""#,
        user_id
    )
    .fetch_all(db())
    .await?;

    send("log.cache.user_tags_update", (user_id.to_string(), tags)).await?;

    Ok(())
}

/// Note that a tag arrived from a signup context, in the encoding orrery uses
/// for the same column, so that the two sides describe the account alike.
async fn remember_signup_tag(user_id: &str, tag: &str) -> Result<(), Error> {
    let held = sqlx::query_scalar!(
        r#"SELECT "signup_tags" FROM "users" WHERE "user_id" = ?"#,
        user_id
    )
    .fetch_optional(db())
    .await?;

    let Some(held) = held else {
        return Ok(());
    };

    let mut names = crate::constant_contact::decode_tags(&held);

    if !names.insert(tag.to_string()) {
        return Ok(());
    }

    let encoded = crate::constant_contact::encode_tags(&names);

    sqlx::query!(
        r#"UPDATE "users" SET "signup_tags" = ? WHERE "user_id" = ?"#,
        encoded,
        user_id
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Remove a tag from a user
pub async fn remove_user_tag(req: &Request) -> Result<(), Error> {
    let (user_id, tag): (String, String) = flexbuffers::from_slice(&req.message.payload)?;

    // Remove the tag
    sqlx::query!(
        r#"DELETE FROM "user_tags" WHERE "user_id" = ? AND "tag" = ?"#,
        user_id,
        tag
    )
    .execute(db())
    .await?;

    // Constant Contact holds some of these tags directly, and derives the
    // "projects participated in" and "library affiliations" fields from others,
    // so any change here may have made their copy stale. Flagged unconditionally
    // rather than testing which tags matter: that test already exists, in the
    // digest the sweep compares before sending, and a second copy of it here
    // would be one more thing to keep in step. An irrelevant tag costs a
    // cleared flag and no request at all.
    crate::constant_contact::mark_dirty(&user_id).await?;

    // Get all current tags and notify log service
    let tags = sqlx::query_scalar!(
        r#"SELECT "tag" FROM "user_tags" WHERE "user_id" = ? ORDER BY "tag""#,
        user_id
    )
    .fetch_all(db())
    .await?;

    send("log.cache.user_tags_update", (user_id, tags)).await?;

    Ok(())
}

/// Get all email addresses for a user
pub async fn get_user_emails(req: &Request) -> Result<Vec<String>, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;

    let emails = sqlx::query_scalar!(
        r#"SELECT "email" FROM "emails" WHERE "user_id" = ? ORDER BY "email""#,
        user_id
    )
    .fetch_all(db())
    .await?;

    Ok(emails)
}

/// Check if a user has a specific email address (case-insensitive)
pub async fn has_user_email(req: &Request) -> Result<bool, Error> {
    let (user_id, email): (String, String) = flexbuffers::from_slice(&req.message.payload)?;

    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!" FROM "emails" WHERE "user_id" = ? AND LOWER("email") = LOWER(?)"#,
        user_id,
        email
    )
    .fetch_one(db())
    .await?;

    Ok(count > 0)
}

/// Set user email addresses (replaces all existing emails)
pub async fn set_user_emails(req: &Request) -> Result<(), Error> {
    let (user_id, emails): (String, Vec<String>) = flexbuffers::from_slice(&req.message.payload)?;

    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

    // Remove all existing emails for this user
    sqlx::query!(r#"DELETE FROM "emails" WHERE "user_id" = ?"#, user_id)
        .execute(&mut tx)
        .await?;

    // Insert new emails with proper hashing
    for email in &emails {
        let hashed = hash_identifier(email);
        let hashed_lower = hash_identifier_lower(email);
        let hashed_capfirst = hash_identifier_capfirst(email);

        sqlx::query!(
            r#"INSERT INTO "emails" ("user_id", "email", "verified", "hash", "hash_lower", "hash_capfirst") VALUES (?, ?, ?, ?, ?, ?)"#,
            user_id,
            email,
            false, // verified - new emails are unverified by default
            hashed,
            hashed_lower,
            hashed_capfirst
        )
        .execute(&mut tx)
        .await?;
    }

    tx.commit().await?;

    // Update user's verified status
    if let Ok(user_ulid) = user_id.parse::<Ulid>() {
        update_user_verified_status(user_ulid).await.ok();
    }

    // Notify log service of email update
    send(
        "log.cache.user_emails_update",
        (user_id.clone(), emails.clone()),
    )
    .await?;

    Ok(())
}

/// Add an email address to a user
pub async fn add_user_email(req: &Request) -> Result<(), Error> {
    let (user_id, email): (String, String) = flexbuffers::from_slice(&req.message.payload)?;

    let hashed = hash_identifier(&email);
    let hashed_lower = hash_identifier_lower(&email);
    let hashed_capfirst = hash_identifier_capfirst(&email);

    // Insert the email (ignore if already exists)
    sqlx::query!(
        r#"INSERT OR IGNORE INTO "emails" ("user_id", "email", "verified", "hash", "hash_lower", "hash_capfirst") VALUES (?, ?, ?, ?, ?, ?)"#,
        user_id,
        email,
        false, // verified - new emails are unverified by default
        hashed,
        hashed_lower,
        hashed_capfirst
    )
    .execute(db())
    .await?;

    // Update user's verified status (new emails are unverified, so this likely won't change status)
    if let Ok(user_ulid) = user_id.parse::<Ulid>() {
        update_user_verified_status(user_ulid).await.ok();
    }

    // Get all current emails and notify log service
    let emails = sqlx::query_scalar!(
        r#"SELECT "email" FROM "emails" WHERE "user_id" = ? ORDER BY "email""#,
        user_id
    )
    .fetch_all(db())
    .await?;

    send("log.cache.user_emails_update", (user_id, emails)).await?;

    Ok(())
}

/// Remove an email address from a user
pub async fn remove_user_email(req: &Request) -> Result<(), Error> {
    let (user_id, email): (String, String) = flexbuffers::from_slice(&req.message.payload)?;

    // Remove the email
    sqlx::query!(
        r#"DELETE FROM "emails" WHERE "user_id" = ? AND "email" = ?"#,
        user_id,
        email
    )
    .execute(db())
    .await?;

    // Update user's verified status (removing verified email could change status)
    if let Ok(user_ulid) = user_id.parse::<Ulid>() {
        update_user_verified_status(user_ulid).await.ok();
    }

    // Get all current emails and notify log service
    let emails = sqlx::query_scalar!(
        r#"SELECT "email" FROM "emails" WHERE "user_id" = ? ORDER BY "email""#,
        user_id
    )
    .fetch_all(db())
    .await?;

    send("log.cache.user_emails_update", (user_id, emails)).await?;

    Ok(())
}

#[derive(serde::Deserialize)]
pub struct SyncRequest {
    pub offset: Option<i64>,
    pub limit: Option<i64>,
}

/// Get user tags for cache synchronization with pagination
/// Returns a vector of (user_id, tag) tuples
/// Request payload: { "offset": 0, "limit": 1000 } (both optional, defaults: offset=0, limit=1000)
pub async fn user_tags(req: &Request) -> Result<Vec<(String, String)>, Error> {
    let sync_req: SyncRequest = if req.message.payload.is_empty() {
        SyncRequest {
            offset: None,
            limit: None,
        }
    } else {
        flexbuffers::from_slice(&req.message.payload)?
    };

    let offset = sync_req.offset.unwrap_or(0);
    let limit = sync_req.limit.unwrap_or(1000);

    let db = db();
    let tags = sqlx::query!(
        r#"SELECT "user_id", "tag" FROM "user_tags" ORDER BY "user_id", "tag" LIMIT ? OFFSET ?"#,
        limit,
        offset
    )
    .fetch_all(db)
    .await?;

    Ok(tags.into_iter().map(|row| (row.user_id, row.tag)).collect())
}

/// Get user emails for cache synchronization with pagination
/// Returns a vector of (user_id, email) tuples
/// Request payload: { "offset": 0, "limit": 1000 } (both optional, defaults: offset=0, limit=1000)
pub async fn user_emails(req: &Request) -> Result<Vec<(String, String)>, Error> {
    let sync_req: SyncRequest = if req.message.payload.is_empty() {
        SyncRequest {
            offset: None,
            limit: None,
        }
    } else {
        flexbuffers::from_slice(&req.message.payload)?
    };

    let offset = sync_req.offset.unwrap_or(0);
    let limit = sync_req.limit.unwrap_or(1000);

    let db = db();
    let emails = sqlx::query!(
        r#"SELECT "user_id", "email" FROM "emails" ORDER BY "user_id", "email" LIMIT ? OFFSET ?"#,
        limit,
        offset
    )
    .fetch_all(db)
    .await?;

    Ok(emails
        .into_iter()
        .map(|row| (row.user_id, row.email))
        .collect())
}

pub async fn get_user_badges(req: &Request) -> Result<Vec<UserBadge>, Error> {
    let user_id: Ulid = flexbuffers::from_slice(&req.message.payload)?;
    let user_id_str = user_id.to_string();

    let badges = sqlx::query!(
        r#"SELECT "badgeclass", "when" FROM "user_badges" WHERE "user_id" = ? ORDER BY "when" DESC"#,
        user_id_str
    )
    .fetch_all(db())
    .await?;

    let mut result = Vec::new();
    for row in badges {
        if let Some(badge) = Badge::from_badgeclass(&row.badgeclass) {
            let timestamp = row.when.clone();
            match TimeString::try_from(row.when) {
                Ok(when) => result.push(UserBadge { badge, when }),
                Err(e) => {
                    eprintln!("Failed to parse badge timestamp '{}': {}", timestamp, e);
                    // Skip this badge if timestamp parsing fails
                    continue;
                }
            }
        } else {
            eprintln!("Unknown badgeclass '{}' found in database", row.badgeclass);
            // Skip this badge if badgeclass is unknown
            continue;
        }
    }

    Ok(result)
}

pub async fn get_user_badge(req: &Request) -> Result<Option<UserBadge>, Error> {
    let params: UserAndBadge = flexbuffers::from_slice(&req.message.payload)?;
    let user_id_str = params.user_id.to_string();
    let badgeclass = params.badge.badgeclass();

    let badge_row = sqlx::query!(
        r#"SELECT "badgeclass", "when" FROM "user_badges" WHERE "user_id" = ? AND "badgeclass" = ?"#,
        user_id_str,
        badgeclass
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = badge_row else {
        return Ok(None);
    };

    match TimeString::try_from(row.when.clone()) {
        Ok(when) => Ok(Some(UserBadge {
            badge: params.badge,
            when,
        })),
        Err(e) => {
            eprintln!("Failed to parse badge timestamp '{}': {}", row.when, e);
            Ok(None)
        }
    }
}

pub async fn grant_user_badge(req: &Request) -> Result<(), Error> {
    let params: UserAndBadge = flexbuffers::from_slice(&req.message.payload)?;

    let now = TimeString::now();
    let badgeclass = params.badge.badgeclass();
    let user_id_str = params.user_id.to_string();

    sqlx::query!(
        r#"insert into "user_badges" ("user_id", "badgeclass", "when")
           values (?, ?, ?)
           on conflict ("user_id", "badgeclass") do nothing"#,
        user_id_str,
        badgeclass,
        now,
    )
    .execute(db())
    .await?;

    // Log the badge granting activity
    log_activity(
        Some(now),
        "badge_earned",
        Some(params.user_id),
        None::<Ulid>,
        Some(badgeclass.to_string()),
    )
    .await?;

    Ok(())
}

pub async fn revoke_user_badge(req: &Request) -> Result<(), Error> {
    let params: UserAndBadge = flexbuffers::from_slice(&req.message.payload)?;

    let badgeclass = params.badge.badgeclass();
    let user_id_str = params.user_id.to_string();

    sqlx::query!(
        r#"DELETE FROM "user_badges" WHERE "user_id" = ? AND "badgeclass" = ?"#,
        user_id_str,
        badgeclass,
    )
    .execute(db())
    .await?;

    // Log the badge revocation activity
    log_activity(
        None,
        "badge_revoked",
        Some(params.user_id),
        None::<Ulid>,
        Some(badgeclass.to_string()),
    )
    .await?;

    Ok(())
}

pub async fn has_user_badge(req: &Request) -> Result<bool, Error> {
    let params: UserAndBadge = flexbuffers::from_slice(&req.message.payload)?;

    let badgeclass = params.badge.badgeclass();
    let user_id_str = params.user_id.to_string();
    let result = sqlx::query!(
        r#"SELECT "badgeclass" FROM "user_badges" WHERE "user_id" = ? AND "badgeclass" = ? LIMIT 1"#,
        user_id_str,
        badgeclass
    )
    .fetch_optional(db())
    .await?;

    Ok(result.is_some())
}

pub async fn get_badge_holders(req: &Request) -> Result<BadgeHoldersResponse, Error> {
    let request: BadgeHoldersRequest = decode_structured_message(req.message.clone())?;

    let page = request.page.unwrap_or(1);
    let limit = request.limit.unwrap_or(50);
    let offset = (page - 1) * limit;

    let badgeclass = request.badge.badgeclass();
    let search_pattern = request.search.as_deref().unwrap_or("");
    let search_like = if search_pattern.is_empty() {
        "%".to_string()
    } else {
        format!("%{}%", search_pattern)
    };

    // Get total count
    let total_count = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) as count
        FROM "user_badges" ub
        INNER JOIN "users" u ON ub."user_id" = u."user_id"
        WHERE ub."badgeclass" = ?
            AND (u."handle" LIKE ? OR u."first_name" LIKE ? OR u."last_name" LIKE ? OR u."primary_email" LIKE ?)
        "#,
        badgeclass,
        search_like,
        search_like,
        search_like,
        search_like
    )
    .fetch_one(db())
    .await?;

    // Get paginated results
    let holders_result = sqlx::query!(
        r#"
        SELECT
            u."user_id",
            u."handle",
            u."first_name",
            u."last_name",
            u."primary_email",
            u."verified" as "verified: bool",
            ub."when"
        FROM "user_badges" ub
        INNER JOIN "users" u ON ub."user_id" = u."user_id"
        WHERE ub."badgeclass" = ?
            AND (u."handle" LIKE ? OR u."first_name" LIKE ? OR u."last_name" LIKE ? OR u."primary_email" LIKE ?)
        ORDER BY ub."when" DESC
        LIMIT ? OFFSET ?
        "#,
        badgeclass,
        search_like,
        search_like,
        search_like,
        search_like,
        limit,
        offset
    )
    .fetch_all(db())
    .await?;

    let mut holders = Vec::with_capacity(holders_result.len());
    for row in holders_result {
        let user_id = row
            .user_id
            .as_ulid()
            .map_err(|e| anyhow::anyhow!("Invalid user ID in database: {}", e))?;

        let when = TimeString::try_from(row.when.clone()).unwrap_or_else(|_| TimeString::now());

        holders.push(BadgeHolderInfo {
            user_id,
            handle: row.handle,
            first_name: row.first_name,
            last_name: row.last_name,
            primary_email: row.primary_email,
            verified: row.verified,
            when,
        });
    }

    Ok(BadgeHoldersResponse {
        holders,
        total_count: total_count as u32,
        page,
        limit,
    })
}

pub async fn user_details_list(req: &Request) -> Result<UserDetailsListResponse, Error> {
    let request: UserDetailsListRequest = decode_structured_message(req.message.clone())?;

    let page = request.page.unwrap_or(1);
    let limit = request.limit.unwrap_or(25);
    let offset = (page - 1) * limit;

    let users_result = sqlx::query!(
        r#"
        SELECT
            "user_id",
            "handle",
            "first_name",
            "last_name",
            "primary_email",
            "is_active" as "is_active: bool",
            "is_staff" as "is_staff: bool",
            "is_superuser" as "is_superuser: bool",
            "is_page_manager" as "is_page_manager: bool",
            "verified" as "verified: bool",
            "date_joined",
            "orcid"
        FROM "users"
        ORDER BY "date_joined" DESC
        LIMIT ? OFFSET ?
        "#,
        limit,
        offset
    )
    .fetch_all(db())
    .await?;

    let total_count_result = sqlx::query!("SELECT COUNT(*) as count FROM users")
        .fetch_one(db())
        .await?;
    let total_count = total_count_result.count as u64;

    let mut users = Vec::new();
    for row in users_result {
        let user_id = row
            .user_id
            .as_ulid()
            .map_err(|e| anyhow::anyhow!("Invalid user ID in database: {}", e))?;

        users.push(UserDetails {
            user_id,
            handle: row.handle,
            first_name: row.first_name,
            last_name: row.last_name,
            primary_email: row.primary_email,
            is_active: row.is_active,
            is_staff: row.is_staff,
            is_superuser: row.is_superuser,
            is_page_manager: row.is_page_manager,
            verified: row.verified,
            date_joined: row.date_joined,
            emails: vec![], // Empty for list operations - use user_details_get for full details
            orcid: row.orcid,
        });
    }

    Ok(UserDetailsListResponse {
        users,
        total_count,
        page,
        limit,
    })
}

/// Search intent derived from the raw search string.
enum UserSearchMode {
    /// No search term supplied — match all rows.
    All,
    /// Search supplied, but no token is long enough for the trigram index
    /// (trigram requires ≥ 3 chars per token). Match nothing.
    NoMatch,
    /// FTS5 MATCH query built from the search term.
    Fts(String),
}

fn user_search_mode(raw: &str) -> UserSearchMode {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return UserSearchMode::All;
    }
    let tokens: Vec<String> = trimmed
        .split_whitespace()
        .filter(|w| w.chars().count() >= 3)
        .map(|w| format!("\"{}\"", w.replace('"', "\"\"")))
        .collect();
    if tokens.is_empty() {
        UserSearchMode::NoMatch
    } else {
        UserSearchMode::Fts(tokens.join(" "))
    }
}

pub async fn user_details_search(req: &Request) -> Result<UserDetailsListResponse, Error> {
    let request: UserDetailsSearchRequest = decode_structured_message(req.message.clone())?;

    let page = request.page.unwrap_or(1);
    let limit = request.limit.unwrap_or(25);
    let offset = (page - 1) * limit;

    let search_raw = request.search.as_deref().unwrap_or("");
    let mode = user_search_mode(search_raw);

    let (role_staff_filter, role_page_manager_filter) = match request.role.as_deref() {
        Some("staff") => (Some(true), None),
        Some("page_manager") => (None, Some(true)),
        Some("user") => (Some(false), Some(false)),
        _ => (None, None),
    };

    let email_verified_filter = match request.email_status.as_deref() {
        Some("verified") => Some(true),
        Some("unverified") => Some(false),
        _ => None,
    };

    if matches!(mode, UserSearchMode::NoMatch) {
        return Ok(UserDetailsListResponse {
            users: Vec::new(),
            total_count: 0,
            page,
            limit,
        });
    }

    let fts_query = match &mode {
        UserSearchMode::Fts(q) => Some(q.as_str()),
        _ => None,
    };

    let users_result = sqlx::query!(
        r#"
        SELECT
            u."user_id",
            u."handle",
            u."first_name",
            u."last_name",
            u."primary_email",
            u."is_active" as "is_active: bool",
            u."is_staff" as "is_staff: bool",
            u."is_superuser" as "is_superuser: bool",
            u."is_page_manager" as "is_page_manager: bool",
            u."verified" as "verified: bool",
            u."date_joined",
            u."orcid"
        FROM "users" u
        WHERE
            (? IS NULL OR u.rowid IN (SELECT rowid FROM users_fts WHERE users_fts MATCH ?))
            AND (? IS NULL OR u."is_staff" = ?)
            AND (? IS NULL OR u."is_page_manager" = ?)
            AND (? IS NULL OR u."verified" = ?)
        ORDER BY u."date_joined" DESC
        LIMIT ? OFFSET ?
        "#,
        fts_query,
        fts_query,
        role_staff_filter,
        role_staff_filter,
        role_page_manager_filter,
        role_page_manager_filter,
        email_verified_filter,
        email_verified_filter,
        limit,
        offset
    )
    .fetch_all(db())
    .await?;

    let total_count_result = sqlx::query!(
        r#"
        SELECT COUNT(*) as count FROM "users" u
        WHERE
            (? IS NULL OR u.rowid IN (SELECT rowid FROM users_fts WHERE users_fts MATCH ?))
            AND (? IS NULL OR u."is_staff" = ?)
            AND (? IS NULL OR u."is_page_manager" = ?)
            AND (? IS NULL OR u."verified" = ?)
        "#,
        fts_query,
        fts_query,
        role_staff_filter,
        role_staff_filter,
        role_page_manager_filter,
        role_page_manager_filter,
        email_verified_filter,
        email_verified_filter
    )
    .fetch_one(db())
    .await?;
    let total_count = total_count_result.count as u64;

    let mut users = Vec::new();
    for row in users_result {
        let user_id = row
            .user_id
            .as_ulid()
            .map_err(|e| anyhow::anyhow!("Invalid user ID in database: {}", e))?;

        users.push(UserDetails {
            user_id,
            handle: row.handle,
            first_name: row.first_name,
            last_name: row.last_name,
            primary_email: row.primary_email,
            is_active: row.is_active,
            is_staff: row.is_staff,
            is_superuser: row.is_superuser,
            is_page_manager: row.is_page_manager,
            verified: row.verified,
            date_joined: row.date_joined,
            emails: vec![], // Empty for list operations - use user_details_get for full details
            orcid: row.orcid,
        });
    }

    Ok(UserDetailsListResponse {
        users,
        total_count,
        page,
        limit,
    })
}

pub async fn user_details_export(req: &Request) -> Result<UserDetailsExportResponse, Error> {
    let request: UserDetailsExportRequest = decode_structured_message(req.message.clone())?;

    let page = request.page.unwrap_or(1);
    let limit = request.limit.unwrap_or(500);
    let offset = (page - 1) * limit;

    let users_result = sqlx::query!(
        r#"
        SELECT
            "user_id",
            "handle",
            "first_name",
            "last_name",
            "primary_email",
            "is_active" as "is_active: bool",
            "is_staff" as "is_staff: bool",
            "is_superuser" as "is_superuser: bool",
            "is_page_manager" as "is_page_manager: bool",
            "verified" as "verified: bool",
            "date_joined",
            "orcid"
        FROM "users"
        WHERE "date_joined" >= ? AND "date_joined" <= ?
        ORDER BY "date_joined" DESC
        LIMIT ? OFFSET ?
        "#,
        request.start_date,
        request.end_date,
        limit,
        offset
    )
    .fetch_all(db())
    .await?;

    // Pre-allocate with exact capacity to avoid reallocations
    let mut users = Vec::with_capacity(users_result.len());
    for row in users_result {
        let user_id = row
            .user_id
            .as_ulid()
            .map_err(|e| anyhow::anyhow!("Invalid user ID in database: {}", e))?;

        users.push(UserDetails {
            user_id,
            handle: row.handle,
            first_name: row.first_name,
            last_name: row.last_name,
            primary_email: row.primary_email,
            is_active: row.is_active,
            is_staff: row.is_staff,
            is_superuser: row.is_superuser,
            is_page_manager: row.is_page_manager,
            verified: row.verified,
            date_joined: row.date_joined,
            emails: vec![], // Empty for export operations
            orcid: row.orcid,
        });
    }

    Ok(UserDetailsExportResponse { users })
}

pub async fn user_details_get(req: &Request) -> Result<UserDetailsResponse, Error> {
    let request: UserDetailsGetRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = request.user_id.to_string();
    let user_result = sqlx::query!(
        r#"
        SELECT
            "user_id",
            "handle",
            "first_name",
            "last_name",
            "primary_email",
            "is_active" as "is_active: bool",
            "is_staff" as "is_staff: bool",
            "is_superuser" as "is_superuser: bool",
            "is_page_manager" as "is_page_manager: bool",
            "verified" as "verified: bool",
            "date_joined",
            "orcid"
        FROM "users"
        WHERE "user_id" = ?
        "#,
        user_id_str
    )
    .fetch_optional(db())
    .await?;

    match user_result {
        Some(row) => {
            let user_id = row
                .user_id
                .as_ulid()
                .map_err(|e| anyhow::anyhow!("Invalid user ID in database: {}", e))?;

            // Fetch user emails
            let emails = fetch_user_emails(&user_id_str).await?;

            let user = UserDetails {
                user_id,
                handle: row.handle,
                first_name: row.first_name,
                last_name: row.last_name,
                primary_email: row.primary_email,
                is_active: row.is_active,
                is_staff: row.is_staff,
                is_superuser: row.is_superuser,
                is_page_manager: row.is_page_manager,
                verified: row.verified,
                date_joined: row.date_joined,
                emails,
                orcid: row.orcid,
            };

            Ok(UserDetailsResponse { user })
        }
        None => Err(anyhow::anyhow!("User not found").into()),
    }
}

pub async fn user_email_info(req: &Request) -> Result<UserEmailInfo, Error> {
    let request: UserEmailInfoRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = request.user_id.to_string();
    let user_result = sqlx::query!(
        r#"
        SELECT
            "user_id",
            "handle",
            "first_name",
            "last_name",
            "primary_email",
            "verified",
            "accept_emails"
        FROM "users"
        WHERE "user_id" = ?
        "#,
        user_id_str
    )
    .fetch_optional(db())
    .await?;

    match user_result {
        Some(row) => {
            let user_id = row
                .user_id
                .as_ulid()
                .map_err(|e| anyhow::anyhow!("Invalid user ID in database: {}", e))?;

            Ok(UserEmailInfo {
                user_id,
                handle: row.handle,
                first_name: row.first_name,
                last_name: row.last_name,
                primary_email: row.primary_email,
                verified: row.verified,
                accept_emails: row.accept_emails != 0,
            })
        }
        None => Err(anyhow::anyhow!("User not found").into()),
    }
}

pub async fn user_details_batch_get(req: &Request) -> Result<UserDetailsBatchGetResponse, Error> {
    let request: UserDetailsBatchGetRequest = decode_structured_message(req.message.clone())?;

    if request.user_ids.is_empty() {
        return Ok(UserDetailsBatchGetResponse { users: Vec::new() });
    }

    // Build query to get users by IDs
    let mut query_builder = sqlx::QueryBuilder::new(
        r#"SELECT "user_id", "primary_email" FROM "users" WHERE "user_id" IN ("#,
    );
    let mut separated = query_builder.separated(", ");
    for user_id in &request.user_ids {
        separated.push_bind(user_id.to_string());
    }
    separated.push_unseparated(")");

    let rows = query_builder
        .build_query_as::<(String, String)>()
        .fetch_all(db())
        .await?;

    let users: Result<Vec<UserNameMapping>, Error> = rows
        .into_iter()
        .map(
            |(user_id_str, primary_email)| -> Result<UserNameMapping, Error> {
                let user_id = user_id_str
                    .as_ulid()
                    .map_err(|e| anyhow::anyhow!("Invalid user ID in database: {}", e))?;
                Ok(UserNameMapping {
                    user_id,
                    primary_email,
                })
            },
        )
        .collect();

    Ok(UserDetailsBatchGetResponse { users: users? })
}

pub async fn user_display_info_batch(req: &Request) -> Result<UserDisplayInfoBatchResponse, Error> {
    let request: UserDisplayInfoBatchRequest = decode_structured_message(req.message.clone())?;

    if request.user_ids.is_empty() {
        return Ok(UserDisplayInfoBatchResponse { users: Vec::new() });
    }

    // Build query to get user display info by IDs. `public` and `protected`
    // come along because this info is always destined for somebody else's
    // screen, and a private user must not be named or pictured there.
    let mut query_builder = sqlx::QueryBuilder::new(
        r#"SELECT "user_id", "handle", "image", "public", "protected" FROM "users" WHERE "user_id" IN ("#,
    );
    let mut separated = query_builder.separated(", ");
    for user_id in &request.user_ids {
        separated.push_bind(user_id.to_string());
    }
    separated.push_unseparated(")");

    let rows = query_builder
        .build_query_as::<(String, String, String, bool, bool)>()
        .fetch_all(db())
        .await?;

    let users: Result<Vec<UserDisplayInfo>, Error> = rows
        .into_iter()
        .map(
            |(user_id_str, handle, image, public, protected)| -> Result<UserDisplayInfo, Error> {
                let user_id = user_id_str
                    .as_ulid()
                    .map_err(|e| anyhow::anyhow!("Invalid user ID in database: {}", e))?;
                let private = !public || protected;
                Ok(UserDisplayInfo {
                    user_id,
                    handle: if private {
                        PRIVATE_DISPLAY_NAME.to_string()
                    } else {
                        handle
                    },
                    image: if private { String::new() } else { image },
                    private,
                })
            },
        )
        .collect();

    Ok(UserDisplayInfoBatchResponse { users: users? })
}

pub async fn user_details_create(req: &Request) -> Result<UserDetailsResponse, Error> {
    let request: UserDetailsCreateRequest = decode_structured_message(req.message.clone())?;

    let user_id = ulid::Ulid::new();

    let (is_staff, is_page_manager) = match request.role.as_str() {
        "staff" => (true, false),
        "page_manager" => (false, true),
        "user" | _ => (false, false),
    };

    let user_id_str = user_id.to_string();
    let password_hash = djangohashers::make_password(&request.password);

    let mut tx = db().begin().await?;

    let result = sqlx::query!(
        r#"INSERT INTO "users" (
            "user_id", "handle", "first_name", "last_name", "primary_email",
            "password", "is_active", "is_staff", "is_superuser", "is_page_manager", "verified",
            "date_joined"
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))"#,
        user_id_str,
        request.handle,
        request.first_name,
        request.last_name,
        request.primary_email,
        password_hash,
        true,
        is_staff,
        false,
        is_page_manager,
        request.verified
    )
    .execute(&mut *tx)
    .await;

    match result {
        Ok(_) => {
            // Add primary email to emails table
            let hashed = hash_identifier(&request.primary_email);
            let hashed_lower = hash_identifier_lower(&request.primary_email);
            let hashed_capfirst = hash_identifier_capfirst(&request.primary_email);

            let email_result = sqlx::query!(
                r#"INSERT INTO "emails" (
                    "user_id", "email", "verified", "hash", "hash_lower", "hash_capfirst"
                ) VALUES (?, ?, ?, ?, ?, ?)"#,
                user_id_str,
                request.primary_email,
                request.verified,
                hashed,
                hashed_lower,
                hashed_capfirst
            )
            .execute(&mut *tx)
            .await;

            if email_result.is_ok() {
                tx.commit().await?;
            } else {
                tx.rollback().await?;
                return Err(anyhow::anyhow!("Failed to create user email record"));
            }
        }
        Err(e) => {
            tx.rollback().await?;
            return Err(anyhow::anyhow!("Failed to create user: {}", e));
        }
    }

    // Fetch user emails for the response
    let emails = vec![UserEmail {
        email: request.primary_email.clone(),
        verified: request.verified,
    }];

    let user = UserDetails {
        user_id,
        handle: request.handle.clone(),
        first_name: request.first_name.clone(),
        last_name: request.last_name.clone(),
        primary_email: request.primary_email,
        is_active: true,
        is_staff,
        is_superuser: false,
        is_page_manager,
        verified: request.verified,
        date_joined: "now".to_string(),
        emails,
        orcid: None,
    };

    Ok(UserDetailsResponse { user })
}

pub async fn user_details_update(req: &Request) -> Result<UserDetailsResponse, Error> {
    let request: UserDetailsUpdateRequest = decode_structured_message(req.message.clone())?;

    let (is_staff, is_page_manager) = match request.role.as_str() {
        "staff" => (true, false),
        "page_manager" => (false, true),
        "user" | _ => (false, false),
    };

    let user_id_str = request.user_id.to_string();

    // Calculate verified status based on email records
    let calculated_verified = calculate_user_verified_status(request.user_id).await?;

    // Handle password update if provided
    let result = if let Some(new_password) = &request.new_password {
        let password_hash = djangohashers::make_password(new_password);
        sqlx::query!(
            r#"UPDATE "users" SET
                "handle" = ?, "first_name" = ?, "last_name" = ?, "primary_email" = ?,
                "password" = ?, "is_active" = ?, "is_staff" = ?, "is_page_manager" = ?, "verified" = ?,
                "orcid" = ?
            WHERE "user_id" = ?"#,
            request.handle,
            request.first_name,
            request.last_name,
            request.primary_email,
            password_hash,
            request.is_active,
            is_staff,
            is_page_manager,
            calculated_verified,
            request.orcid,
            user_id_str
        )
        .execute(db())
        .await
    } else {
        sqlx::query!(
            r#"UPDATE "users" SET
                "handle" = ?, "first_name" = ?, "last_name" = ?, "primary_email" = ?,
                "is_active" = ?, "is_staff" = ?, "is_page_manager" = ?, "verified" = ?,
                "orcid" = ?
            WHERE "user_id" = ?"#,
            request.handle,
            request.first_name,
            request.last_name,
            request.primary_email,
            request.is_active,
            is_staff,
            is_page_manager,
            calculated_verified,
            request.orcid,
            user_id_str
        )
        .execute(db())
        .await
    };

    match result {
        Ok(result) => {
            if result.rows_affected() == 0 {
                return Err(anyhow::anyhow!("User not found").into());
            }

            // Fetch the updated user data from database
            let updated_user = sqlx::query!(
                r#"SELECT "date_joined", "is_superuser" FROM "users" WHERE "user_id" = ?"#,
                user_id_str
            )
            .fetch_one(db())
            .await?;

            // Fetch user emails for the response
            let emails = fetch_user_emails(&user_id_str).await?;

            let user = UserDetails {
                user_id: request.user_id,
                handle: request.handle,
                first_name: request.first_name,
                last_name: request.last_name,
                primary_email: request.primary_email,
                is_active: request.is_active,
                is_staff,
                is_superuser: updated_user.is_superuser != 0,
                is_page_manager,
                verified: calculated_verified,
                date_joined: updated_user.date_joined,
                emails,
                orcid: request.orcid,
            };

            Ok(UserDetailsResponse { user })
        }
        Err(e) => Err(anyhow::anyhow!("Failed to update user: {}", e).into()),
    }
}

pub async fn user_details_delete(req: &Request) -> Result<UserDetailsDeleteResponse, Error> {
    let request: UserDetailsDeleteRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = request.user_id.to_string();
    let result = sqlx::query!(r#"DELETE FROM "users" WHERE "user_id" = ?"#, user_id_str)
        .execute(db())
        .await;

    match result {
        Ok(result) => {
            let success = result.rows_affected() > 0;
            Ok(UserDetailsDeleteResponse { success })
        }
        Err(e) => Err(anyhow::anyhow!("Failed to delete user: {}", e).into()),
    }
}

pub async fn admin_password_reset(req: &Request) -> Result<AdminPasswordResetResponse, Error> {
    let request: AdminPasswordResetRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = request.user_id.to_string();

    // Verify the user exists and the email belongs to them
    let email_check = sqlx::query!(
        r#"SELECT u.user_id, u.handle, e.email, e.verified
           FROM users u
           JOIN emails e ON u.user_id = e.user_id
           WHERE u.user_id = ? AND e.email = ?"#,
        user_id_str,
        request.email
    )
    .fetch_optional(db())
    .await?;

    match email_check {
        Some(row) => {
            if !row.verified {
                return Ok(AdminPasswordResetResponse {
                    success: false,
                    message: "Cannot send password reset to unverified email address".to_string(),
                });
            }

            // TODO: Implement actual email sending logic
            // For now, just return success with a placeholder message
            // In a real implementation, you would:
            // 1. Generate a secure password reset token
            // 2. Store it in a password_reset_tokens table with expiration
            // 3. Send email with reset link
            // 4. Handle the reset link in a separate endpoint

            log_activity(
                None,
                "password-reset-email-sent",
                request.user_id,
                None,
                format!("email={}", request.email),
            )
            .await
            .ok();

            Ok(AdminPasswordResetResponse {
                success: true,
                message: format!("Password reset email sent to {}", request.email),
            })
        }
        None => Ok(AdminPasswordResetResponse {
            success: false,
            message: "User not found or email does not belong to this user".to_string(),
        }),
    }
}

pub async fn admin_email_add(req: &Request) -> Result<UserEmailResponse, Error> {
    let request: UserEmailAddRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = request.user_id.to_string();

    // Check if user exists
    let user_exists = sqlx::query!(
        r#"SELECT "user_id" FROM "users" WHERE "user_id" = ?"#,
        user_id_str
    )
    .fetch_optional(db())
    .await?;

    if user_exists.is_none() {
        return Ok(UserEmailResponse {
            success: false,
            message: "User not found".to_string(),
            emails: vec![],
        });
    }

    // Check if email already exists
    let email_exists = sqlx::query!(
        r#"SELECT "email" FROM "emails" WHERE "email" = ?"#,
        request.email
    )
    .fetch_optional(db())
    .await?;

    if email_exists.is_some() {
        return Ok(UserEmailResponse {
            success: false,
            message: "Email address is already in use".to_string(),
            emails: vec![],
        });
    }

    // Add the email
    let hashed = hash_identifier(&request.email);
    let hashed_lower = hash_identifier_lower(&request.email);
    let hashed_capfirst = hash_identifier_capfirst(&request.email);

    let result = sqlx::query!(
        r#"INSERT INTO "emails" (
            "user_id", "email", "verified", "hash", "hash_lower", "hash_capfirst"
        ) VALUES (?, ?, ?, ?, ?, ?)"#,
        user_id_str,
        request.email,
        false, // New emails start unverified
        hashed,
        hashed_lower,
        hashed_capfirst
    )
    .execute(db())
    .await;

    match result {
        Ok(_) => {
            // Return updated emails list
            let emails = fetch_user_emails(&user_id_str).await?;
            log_activity(
                None,
                "email-added",
                request.user_id,
                None,
                format!("email={}", request.email),
            )
            .await
            .ok();

            Ok(UserEmailResponse {
                success: true,
                message: format!("Email {} added successfully", request.email),
                emails,
            })
        }
        Err(e) => Ok(UserEmailResponse {
            success: false,
            message: format!("Failed to add email: {}", e),
            emails: vec![],
        }),
    }
}

pub async fn admin_email_update(req: &Request) -> Result<UserEmailResponse, Error> {
    let request: UserEmailUpdateRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = request.user_id.to_string();

    // Check if new email is already in use by someone else
    let email_exists = sqlx::query!(
        r#"SELECT "user_id" FROM "emails" WHERE "email" = ? AND "user_id" != ?"#,
        request.new_email,
        user_id_str
    )
    .fetch_optional(db())
    .await?;

    if email_exists.is_some() {
        return Ok(UserEmailResponse {
            success: false,
            message: "Email address is already in use".to_string(),
            emails: vec![],
        });
    }

    // Update the email
    let hashed = hash_identifier(&request.new_email);
    let hashed_lower = hash_identifier_lower(&request.new_email);
    let hashed_capfirst = hash_identifier_capfirst(&request.new_email);

    let result = sqlx::query!(
        r#"UPDATE "emails" SET
            "email" = ?, "hash" = ?, "hash_lower" = ?, "hash_capfirst" = ?, "verified" = ?
        WHERE "user_id" = ? AND "email" = ?"#,
        request.new_email,
        hashed,
        hashed_lower,
        hashed_capfirst,
        false, // Reset verified status when email changes
        user_id_str,
        request.old_email
    )
    .execute(db())
    .await;

    match result {
        Ok(result) => {
            if result.rows_affected() == 0 {
                return Ok(UserEmailResponse {
                    success: false,
                    message: "Email not found".to_string(),
                    emails: vec![],
                });
            }

            // Return updated emails list
            let emails = fetch_user_emails(&user_id_str).await?;
            log_activity(
                None,
                "email-updated",
                request.user_id,
                None,
                format!("old={}, new={}", request.old_email, request.new_email),
            )
            .await
            .ok();

            Ok(UserEmailResponse {
                success: true,
                message: format!(
                    "Email updated from {} to {}",
                    request.old_email, request.new_email
                ),
                emails,
            })
        }
        Err(e) => Ok(UserEmailResponse {
            success: false,
            message: format!("Failed to update email: {}", e),
            emails: vec![],
        }),
    }
}

pub async fn admin_email_delete(req: &Request) -> Result<UserEmailResponse, Error> {
    let request: UserEmailDeleteRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = request.user_id.to_string();

    // Check if this is the user's only email or primary email
    let email_count = sqlx::query!(
        r#"SELECT COUNT(*) as count FROM "emails" WHERE "user_id" = ?"#,
        user_id_str
    )
    .fetch_one(db())
    .await?;

    if email_count.count <= 1 {
        return Ok(UserEmailResponse {
            success: false,
            message: "Cannot delete the user's only email address".to_string(),
            emails: vec![],
        });
    }

    // Check if this is the primary email
    let is_primary = sqlx::query!(
        r#"SELECT "primary_email" FROM "users" WHERE "user_id" = ?"#,
        user_id_str
    )
    .fetch_optional(db())
    .await?;

    if let Some(user) = is_primary {
        if user.primary_email == request.email {
            return Ok(UserEmailResponse {
                success: false,
                message: "Cannot delete the user's primary email address".to_string(),
                emails: vec![],
            });
        }
    }

    // Delete the email
    let result = sqlx::query!(
        r#"DELETE FROM "emails" WHERE "user_id" = ? AND "email" = ?"#,
        user_id_str,
        request.email
    )
    .execute(db())
    .await;

    match result {
        Ok(result) => {
            if result.rows_affected() == 0 {
                return Ok(UserEmailResponse {
                    success: false,
                    message: "Email not found".to_string(),
                    emails: vec![],
                });
            }

            // Update user's overall verified status based on remaining email records
            update_user_verified_status(request.user_id).await.ok(); // Ignore errors for backwards compatibility

            // Return updated emails list
            let emails = fetch_user_emails(&user_id_str).await?;
            log_activity(
                None,
                "email-deleted",
                request.user_id,
                None,
                format!("email={}", request.email),
            )
            .await
            .ok();

            Ok(UserEmailResponse {
                success: true,
                message: format!("Email {} deleted successfully", request.email),
                emails,
            })
        }
        Err(e) => Ok(UserEmailResponse {
            success: false,
            message: format!("Failed to delete email: {}", e),
            emails: vec![],
        }),
    }
}

pub async fn admin_email_toggle_verified(req: &Request) -> Result<UserDetailsResponse, Error> {
    let request: UserEmailToggleVerifiedRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = request.user_id.to_string();

    // Get current verified status
    let current_status = sqlx::query!(
        r#"SELECT "verified" FROM "emails" WHERE "user_id" = ? AND "email" = ?"#,
        user_id_str,
        request.email
    )
    .fetch_optional(db())
    .await?;

    match current_status {
        Some(row) => {
            let new_status = !row.verified;

            // Update verified status
            let result = sqlx::query!(
                r#"UPDATE "emails" SET "verified" = ? WHERE "user_id" = ? AND "email" = ?"#,
                new_status,
                user_id_str,
                request.email
            )
            .execute(db())
            .await;

            match result {
                Ok(_) => {
                    // Update user's overall verified status based on email records
                    update_user_verified_status(request.user_id).await.ok(); // Ignore errors for backwards compatibility

                    log_activity(
                        None,
                        "email-verification-toggled",
                        request.user_id,
                        None,
                        format!("email={}, verified={}", request.email, new_status),
                    )
                    .await
                    .ok();

                    // Fetch complete user data (similar to user_details_get but without separate email fetch)
                    let user_result = sqlx::query!(
                        r#"
                        SELECT
                            "user_id",
                            "handle",
                            "first_name",
                            "last_name",
                            "primary_email",
                            "is_active" as "is_active: bool",
                            "is_staff" as "is_staff: bool",
                            "is_superuser" as "is_superuser: bool",
                            "is_page_manager" as "is_page_manager: bool",
                            "verified" as "verified: bool",
                            "date_joined",
                            "orcid"
                        FROM "users"
                        WHERE "user_id" = ?
                        "#,
                        user_id_str
                    )
                    .fetch_one(db())
                    .await?;

                    let user_id = user_result
                        .user_id
                        .as_ulid()
                        .map_err(|e| anyhow::anyhow!("Invalid user ID in database: {}", e))?;

                    // Fetch updated emails list
                    let emails = fetch_user_emails(&user_id_str).await?;

                    let user = UserDetails {
                        user_id,
                        handle: user_result.handle,
                        first_name: user_result.first_name,
                        last_name: user_result.last_name,
                        primary_email: user_result.primary_email,
                        is_active: user_result.is_active,
                        is_staff: user_result.is_staff,
                        is_superuser: user_result.is_superuser,
                        is_page_manager: user_result.is_page_manager,
                        verified: user_result.verified,
                        date_joined: user_result.date_joined,
                        emails,
                        orcid: user_result.orcid,
                    };

                    Ok(UserDetailsResponse { user })
                }
                Err(e) => Err(anyhow::anyhow!("Failed to toggle verification: {}", e).into()),
            }
        }
        None => Err(anyhow::anyhow!("Email not found").into()),
    }
}

// Helper function to fetch user emails
async fn fetch_user_emails(user_id_str: &str) -> Result<Vec<UserEmail>, Error> {
    let emails_result = sqlx::query!(
        r#"SELECT "email", "verified" FROM "emails" WHERE "user_id" = ? ORDER BY "email""#,
        user_id_str
    )
    .fetch_all(db())
    .await?;

    Ok(emails_result
        .into_iter()
        .map(|row| UserEmail {
            email: row.email,
            verified: row.verified,
        })
        .collect())
}

/// Bulk sync page manager status - sets the flag for all provided user IDs
/// and clears it for everyone else
pub async fn page_manager_sync(req: &Request) -> Result<PageManagerSyncResponse, Error> {
    let request: PageManagerSyncRequest = decode_structured_message(req.message.clone())?;

    // First, clear all page manager flags
    sqlx::query!(r#"UPDATE "users" SET "is_page_manager" = 0"#)
        .execute(db())
        .await?;

    let mut updated_count = 0u64;

    // Set page manager flag for the provided user IDs
    if !request.user_ids.is_empty() {
        for user_id in request.user_ids {
            let user_id_str = user_id.to_string();
            let result = sqlx::query!(
                r#"UPDATE "users" SET "is_page_manager" = 1 WHERE "user_id" = ?"#,
                user_id_str
            )
            .execute(db())
            .await?;

            updated_count += result.rows_affected();
        }
    }

    Ok(PageManagerSyncResponse { updated_count })
}

/// Update page manager status for a single user
pub async fn page_manager_update(req: &Request) -> Result<PageManagerUpdateResponse, Error> {
    let request: PageManagerUpdateRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = request.user_id.to_string();
    let is_page_manager = if request.is_page_manager { 1 } else { 0 };

    let result = sqlx::query!(
        r#"UPDATE "users" SET "is_page_manager" = ? WHERE "user_id" = ?"#,
        is_page_manager,
        user_id_str
    )
    .execute(db())
    .await;

    match result {
        Ok(result) => {
            let success = result.rows_affected() > 0;
            Ok(PageManagerUpdateResponse { success })
        }
        Err(e) => Err(anyhow::anyhow!("Failed to update page manager status: {}", e).into()),
    }
}
