use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{BasicInfo, Login, Signup},
    service::{log_activity, trigger_email, TriggerEvent},
    shared::{db, hash_identifier, hash_identifier_capfirst, hash_identifier_lower},
    ulid::AsUlid,
};
use ulid::Ulid;

#[derive(Debug)]
struct UserWithPassword {
    user_id: String,
    handle: String,
    password: String,
    is_staff: i64,
    is_superuser: i64,
    public: i64,
    protected: i64,
    point: Option<String>,
}

/// Sign the current session up for a new account
pub async fn signup(req: &Request) -> Result<Option<BasicInfo>, Error> {
    let form: Signup = flexbuffers::from_slice(&req.message.payload)?;

    let user_id = Ulid::new();
    let user_id_str = user_id.to_string();
    let handle = form.handle;
    let email = form.email;
    let password = if form.disabled_password {
        // Store Django-style disabled password marker directly
        "!".to_string()
    } else {
        // Hash the user's password
        djangohashers::make_password(&form.password)
    };
    let newsletter = form.newsletter;
    let zipcode = form.zipcode;
    let protected = form.protected;
    let point = form.point;

    let mut tx = db().begin().await?;

    // We're letting users and emails start as verified, for now,
    // unless some criteria are met which make them less trusted
    // initially.

    let mut verified = true;

    if form.tags.iter().any(|s| s == "evolveme") {
        verified = false;
    }

    sqlx::query!(
        r#"insert into "users" (
          "user_id",
          "handle",
          "primary_email",
          "verified",
          "password",
          "newsletter",
          "zipcode",
          "protected",
          "point"
        )
        values (
          ?1,
          ?3,
          ?2,
          ?9,
          ?4,
          ?5,
          ?6,
          ?7,
          GeomFromGeoJSON(?8)
        )"#,
        user_id_str,
        email,
        handle,
        password,
        newsletter,
        zipcode,
        protected,
        point,
        verified,
    )
    .execute(&mut *tx)
    .await?;

    let hashed = hash_identifier(&email);
    let hashed_lower = hash_identifier_lower(&email);
    let hashed_capfirst = hash_identifier_capfirst(&email);

    sqlx::query!(
        r#"insert into "emails" (
          "user_id",
          "email",
          "verified",
          "hash",
          "hash_lower",
          "hash_capfirst"
        )
        values (
          ?,
          ?,
          ?,
          ?,
          ?,
          ?
        )"#,
        user_id_str,
        email,
        verified,
        hashed,
        hashed_lower,
        hashed_capfirst,
    )
    .execute(&mut *tx)
    .await?;

    for tag in form.tags.iter() {
        let tag = tag.to_lowercase();

        sqlx::query!(
            r#"insert into "user_tags" (
              "user_id",
              "tag"
            )
            values (
              ?,
              ?
            )
            on conflict ("user_id", "tag") do nothing"#,
            user_id_str,
            tag,
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    log_activity(None, "signup", user_id, None, None).await.ok();

    trigger_email(
        TriggerEvent::UserWelcome,
        user_id,
        Some(&[("handle".to_string(), handle.clone())]),
    )
    .await
    .ok();

    Ok(Some(BasicInfo {
        user_id: user_id,
        handle,
        is_staff: false,
        is_superuser: false,
        public: true,
        protected,
        needs_point: point.is_none(),
    }))
}

pub async fn login(req: &Request) -> Result<Option<BasicInfo>, Error> {
    let form: Login = flexbuffers::from_slice(&req.message.payload)?;

    // Look up user by email using query_as
    let user_record = sqlx::query_as!(
        UserWithPassword,
        r#"select u.user_id, u.handle, u.password, u.is_staff, u.is_superuser, u.public, u.protected,
           AsText(u.point) as "point: String"
           from users u
           join emails e on u.user_id = e.user_id
           where e.email = ?
           limit 1"#,
        form.email
    )
    .fetch_optional(db())
    .await?;

    if let Some(user) = user_record {
        // Verify password
        if djangohashers::check_password(&form.password, &user.password).unwrap_or(false) {
            // Log successful login
            let user_id = user.user_id.as_ulid().ok();
            log_activity(None, "login", user_id, None, None).await.ok();

            // Return BasicInfo
            Ok(Some(BasicInfo {
                user_id: user.user_id.as_ulid()?,
                handle: user.handle,
                is_staff: user.is_staff != 0,
                is_superuser: user.is_superuser != 0,
                public: user.public != 0,
                protected: user.protected != 0,
                needs_point: user.point.as_ref().map_or(true, |p| p.trim().is_empty()),
            }))
        } else {
            // Log failed login attempt
            let user_id = user.user_id.as_ulid().ok();
            log_activity(None, "login-failed", user_id, None, None)
                .await
                .ok();
            Ok(None)
        }
    } else {
        // User not found
        Ok(None)
    }
}
