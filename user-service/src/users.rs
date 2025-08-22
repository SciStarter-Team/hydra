use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::BasicInfo,
    service::send,
    shared::{db, hash_identifier, hash_identifier_capfirst, hash_identifier_lower},
    transaction::Immediate,
    ulid::AsUlid,
};

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
    let username: String = flexbuffers::from_slice(&req.message.payload)?;

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
        where "username" = ?"#,
        username,
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
