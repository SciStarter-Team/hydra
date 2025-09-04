use anyhow::Error;
use async_nats::service::Request;
use common::{
    badge::Badge,
    message::user::{AdminUser, AdminUserListRequest, AdminUserListResponse, AdminUserSearchRequest, AdminUserCreateRequest, AdminUserUpdateRequest, AdminUserDeleteRequest, AdminUserGetRequest, AdminUserResponse, AdminUserDeleteResponse, BasicInfo, UserAndBadge, UserBadge, PageManagerSyncRequest, PageManagerSyncResponse, PageManagerUpdateRequest, PageManagerUpdateResponse},
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
        })
    }
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
          u."protected" as "protected: bool"
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

pub async fn basic_info_by_user_id(req: &Request) -> Result<Option<BasicInfo>, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;

    let result = sqlx::query_as!(
        BasicInfoDb,
        r#"select
          "user_id",
          "handle",
          "is_staff" as "is_staff: bool",
          "is_superuser" as "is_superuser: bool",
          "public" as "public: bool",
          "protected" as "protected: bool"
        from "users"
        where "user_id" = ?"#,
        user_id,
    )
    .fetch_optional(db())
    .await?;

    match result {
        Some(db_info) => Ok(Some(db_info.try_into()?)),
        None => Ok(None),
    }
}

pub async fn search_by_handle(req: &Request) -> Result<Vec<BasicInfo>, Error> {
    let search_term: String = flexbuffers::from_slice(&req.message.payload)?;

    let results = sqlx::query_as!(
        BasicInfoDb,
        r#"select
          "user_id",
          "handle",
          "is_staff" as "is_staff: bool",
          "is_superuser" as "is_superuser: bool",
          "public" as "public: bool",
          "protected" as "protected: bool"
        from "users"
        where "handle" LIKE ? || '%' 
          AND "public" = 1 
          AND "protected" = 0
        order by "handle""#,
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

    // Insert the tag (ignore if already exists)
    sqlx::query!(
        r#"INSERT OR IGNORE INTO "user_tags" ("user_id", "tag") VALUES (?, ?)"#,
        user_id,
        tag
    )
    .execute(db())
    .await?;

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
        r#"SELECT "badgeclass", "when", "image" FROM "user_badges" WHERE "user_id" = ? ORDER BY "when" DESC"#,
        user_id_str
    )
    .fetch_all(db())
    .await?;

    let mut result = Vec::new();
    for row in badges {
        if let Some(badge) = Badge::from_badgeclass(&row.badgeclass) {
            let timestamp = row.when.clone();
            match TimeString::try_from(row.when) {
                Ok(when) => result.push(UserBadge {
                    badge,
                    when,
                    image: row.image,
                }),
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

pub async fn grant_user_badge(req: &Request) -> Result<(), Error> {
    let params: UserAndBadge = flexbuffers::from_slice(&req.message.payload)?;

    let now = TimeString::now();
    let badgeclass = params.badge.badgeclass();
    let user_id_str = params.user_id.to_string();

    sqlx::query!(
        r#"insert into "user_badges" ("user_id", "badgeclass", "when", "image")
           values (?, ?, ?, ?)
           on conflict ("user_id", "badgeclass") do nothing"#,
        user_id_str,
        badgeclass,
        now,
        None::<String>,
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

pub async fn admin_user_list(req: &Request) -> Result<AdminUserListResponse, Error> {
    let request: AdminUserListRequest = decode_structured_message(req.message.clone())?;
    
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
            "date_joined"
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
        let user_id = row.user_id.as_ulid()
            .map_err(|e| anyhow::anyhow!("Invalid user ID in database: {}", e))?;

        users.push(AdminUser {
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
        });
    }

    Ok(AdminUserListResponse {
        users,
        total_count,
        page,
        limit,
    })
}

pub async fn admin_user_search(req: &Request) -> Result<AdminUserListResponse, Error> {
    let request: AdminUserSearchRequest = decode_structured_message(req.message.clone())?;
    
    let page = request.page.unwrap_or(1);
    let limit = request.limit.unwrap_or(25);
    let offset = (page - 1) * limit;

    let search_pattern = request.search.as_deref().unwrap_or("");
    let search_like = format!("%{}%", search_pattern);
    
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
            "date_joined"
        FROM "users"
        WHERE 
            (? = '' OR "first_name" LIKE ? OR "last_name" LIKE ? OR "handle" LIKE ? OR "primary_email" LIKE ?)
            AND (? IS NULL OR "is_staff" = ?)
            AND (? IS NULL OR "is_page_manager" = ?)
            AND (? IS NULL OR "verified" = ?)
        ORDER BY "date_joined" DESC
        LIMIT ? OFFSET ?
        "#,
        search_pattern,
        search_like,
        search_like, 
        search_like,
        search_like,
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
        SELECT COUNT(*) as count FROM "users"
        WHERE 
            (? = '' OR "first_name" LIKE ? OR "last_name" LIKE ? OR "handle" LIKE ? OR "primary_email" LIKE ?)
            AND (? IS NULL OR "is_staff" = ?)
            AND (? IS NULL OR "is_page_manager" = ?)
            AND (? IS NULL OR "verified" = ?)
        "#,
        search_pattern,
        search_like,
        search_like, 
        search_like,
        search_like,
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
        let user_id = row.user_id.as_ulid()
            .map_err(|e| anyhow::anyhow!("Invalid user ID in database: {}", e))?;

        users.push(AdminUser {
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
        });
    }

    Ok(AdminUserListResponse {
        users,
        total_count,
        page,
        limit,
    })
}

pub async fn admin_user_get(req: &Request) -> Result<AdminUserResponse, Error> {
    let request: AdminUserGetRequest = decode_structured_message(req.message.clone())?;

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
            "date_joined"
        FROM "users"
        WHERE "user_id" = ?
        "#,
        user_id_str
    )
    .fetch_optional(db())
    .await?;

    match user_result {
        Some(row) => {
            let user_id = row.user_id.as_ulid()
                .map_err(|e| anyhow::anyhow!("Invalid user ID in database: {}", e))?;

            let user = AdminUser {
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
            };

            Ok(AdminUserResponse { user })
        }
        None => Err(anyhow::anyhow!("User not found").into()),
    }
}

pub async fn admin_user_create(req: &Request) -> Result<AdminUserResponse, Error> {
    let request: AdminUserCreateRequest = decode_structured_message(req.message.clone())?;
    
    let user_id = ulid::Ulid::new();
    
    let (is_staff, is_page_manager) = match request.role.as_str() {
        "staff" => (true, false),
        "page_manager" => (false, true),
        "user" | _ => (false, false),
    };

    let user_id_str = user_id.to_string();
    let result = sqlx::query!(
        r#"INSERT INTO "users" (
            "user_id", "handle", "first_name", "last_name", "primary_email", 
            "is_active", "is_staff", "is_superuser", "is_page_manager", "verified",
            "date_joined"
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))"#,
        user_id_str,
        request.handle,
        request.first_name,
        request.last_name,
        request.primary_email,
        true,
        is_staff,
        false,
        is_page_manager,
        request.verified
    )
    .execute(db())
    .await;

    match result {
        Ok(_) => {
            let user = AdminUser {
                user_id,
                handle: request.handle,
                first_name: request.first_name,
                last_name: request.last_name,
                primary_email: request.primary_email,
                is_active: true,
                is_staff,
                is_superuser: false,
                is_page_manager,
                verified: request.verified,
                date_joined: "now".to_string(),
            };

            Ok(AdminUserResponse { user })
        }
        Err(e) => Err(anyhow::anyhow!("Failed to create user: {}", e).into()),
    }
}

pub async fn admin_user_update(req: &Request) -> Result<AdminUserResponse, Error> {
    let request: AdminUserUpdateRequest = decode_structured_message(req.message.clone())?;
    
    let (is_staff, is_page_manager) = match request.role.as_str() {
        "staff" => (true, false),
        "page_manager" => (false, true),
        "user" | _ => (false, false),
    };

    let user_id_str = request.user_id.to_string();
    let result = sqlx::query!(
        r#"UPDATE "users" SET
            "handle" = ?, "first_name" = ?, "last_name" = ?, "primary_email" = ?,
            "is_active" = ?, "is_staff" = ?, "is_page_manager" = ?, "verified" = ?
        WHERE "user_id" = ?"#,
        request.handle,
        request.first_name,
        request.last_name,
        request.primary_email,
        request.is_active,
        is_staff,
        is_page_manager,
        request.verified,
        user_id_str
    )
    .execute(db())
    .await;

    match result {
        Ok(result) => {
            if result.rows_affected() == 0 {
                return Err(anyhow::anyhow!("User not found").into());
            }

            let user = AdminUser {
                user_id: request.user_id,
                handle: request.handle,
                first_name: request.first_name,
                last_name: request.last_name,
                primary_email: request.primary_email,
                is_active: request.is_active,
                is_staff,
                is_superuser: false,
                is_page_manager,
                verified: request.verified,
                date_joined: "unknown".to_string(),
            };

            Ok(AdminUserResponse { user })
        }
        Err(e) => Err(anyhow::anyhow!("Failed to update user: {}", e).into()),
    }
}

pub async fn admin_user_delete(req: &Request) -> Result<AdminUserDeleteResponse, Error> {
    let request: AdminUserDeleteRequest = decode_structured_message(req.message.clone())?;

    let user_id_str = request.user_id.to_string();
    let result = sqlx::query!(
        r#"DELETE FROM "users" WHERE "user_id" = ?"#,
        user_id_str
    )
    .execute(db())
    .await;

    match result {
        Ok(result) => {
            let success = result.rows_affected() > 0;
            Ok(AdminUserDeleteResponse { success })
        }
        Err(e) => Err(anyhow::anyhow!("Failed to delete user: {}", e).into()),
    }
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
