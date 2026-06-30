// User matching for API participation recording
// Returns None when user doesn't exist (API caller handles this by returning success/discard)

use async_nats::service::Request;
use common::message::user::{
    AiOptedOutEntry, AiOptedOutRequest, AiOptedOutResponse, ApiCreateUserRequest,
    ApiCreateUserResponse, IdentifierType, ProfileIdLookupRequest, ProfileIdLookupResponse,
    ProfileIdLookupType, UserApiKeyRequest, UserApiKeyResponse, UserMatchRequest,
    UserMatchResponse, UserOpaqueIdRequest, UserOpaqueIdResponse, ValidateApiKeyRequest,
    ValidateApiKeyResponse,
};
use common::{service::decode_structured_message, shared::db};

/// Match user by identifier (hashed email, opaque ID, email, or link token)
pub async fn match_user(req: &Request) -> Result<UserMatchResponse, anyhow::Error> {
    let request: UserMatchRequest = decode_structured_message(req.message.clone())?;

    let user_id = match request.identifier_type {
        IdentifierType::Hashed => match_by_hashed_email(&request.identifier).await?,
        IdentifierType::Opaque => match_by_opaque_id(&request.identifier).await?,
        IdentifierType::Email => match_by_email(&request.identifier).await?,
        IdentifierType::LinkToken => match_by_link_token(&request.identifier).await?,
        IdentifierType::Ulid => match_by_ulid(&request.identifier).await?,
    };

    Ok(UserMatchResponse { user_id })
}

/// Match user by hashed email
/// Checks hash, hash_lower, and hash_capfirst variants
async fn match_by_hashed_email(hash: &str) -> Result<Option<String>, anyhow::Error> {
    let row = sqlx::query!(
        "SELECT user_id FROM emails
         WHERE hash = ? OR hash_lower = ? OR hash_capfirst = ?
         LIMIT 1",
        hash,
        hash,
        hash
    )
    .fetch_optional(db())
    .await?;

    Ok(row.map(|r| r.user_id))
}

/// Match user by opaque_id
async fn match_by_opaque_id(opaque_id: &str) -> Result<Option<String>, anyhow::Error> {
    let row = sqlx::query!(
        "SELECT user_id FROM users WHERE opaque_id = ? LIMIT 1",
        opaque_id
    )
    .fetch_optional(db())
    .await?;

    Ok(row.map(|r| r.user_id))
}

/// Match user by plaintext email
async fn match_by_email(email: &str) -> Result<Option<String>, anyhow::Error> {
    let row = sqlx::query!("SELECT user_id FROM emails WHERE email = ? LIMIT 1", email)
        .fetch_optional(db())
        .await?;

    Ok(row.map(|r| r.user_id))
}

/// Match user by link token
async fn match_by_link_token(token: &str) -> Result<Option<String>, anyhow::Error> {
    let row = sqlx::query!(
        "SELECT user_id FROM link_tokens WHERE token = ? LIMIT 1",
        token
    )
    .fetch_optional(db())
    .await?;

    Ok(row.map(|r| r.user_id))
}

/// Match user by ULID. In hydra the user's ULID is its `user_id` (this is the
/// same value as orrery's `User.ulid`, which is the cross-system identifier).
async fn match_by_ulid(ulid: &str) -> Result<Option<String>, anyhow::Error> {
    let row = sqlx::query!("SELECT user_id FROM users WHERE user_id = ? LIMIT 1", ulid)
        .fetch_optional(db())
        .await?;

    Ok(row.map(|r| r.user_id))
}

/// Generate a URL-safe random API key (similar to Python's secrets.token_urlsafe(64))
fn generate_api_key() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..64).map(|_| rng.r#gen()).collect();

    // Use URL-safe base64 encoding (no padding)
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes)
}

/// Get user's API key, generating one if it doesn't exist
pub async fn get_api_key(req: &Request) -> Result<UserApiKeyResponse, anyhow::Error> {
    let request: UserApiKeyRequest = decode_structured_message(req.message.clone())?;

    let row = sqlx::query!(
        "SELECT api_key FROM users WHERE user_id = ?",
        request.user_id
    )
    .fetch_one(db())
    .await?;

    let api_key = if let Some(existing_key) = row.api_key {
        existing_key
    } else {
        // Generate a new API key
        let new_key = generate_api_key();

        // Save it to the database
        sqlx::query!(
            "UPDATE users SET api_key = ? WHERE user_id = ?",
            new_key,
            request.user_id
        )
        .execute(db())
        .await?;

        new_key
    };

    Ok(UserApiKeyResponse { api_key })
}

/// Get user's opaque_id for external project integration
pub async fn get_opaque_id(req: &Request) -> Result<UserOpaqueIdResponse, anyhow::Error> {
    let request: UserOpaqueIdRequest = decode_structured_message(req.message.clone())?;

    let row = sqlx::query!(
        "SELECT opaque_id FROM users WHERE user_id = ?",
        request.user_id
    )
    .fetch_one(db())
    .await?;

    Ok(UserOpaqueIdResponse {
        opaque_id: row.opaque_id,
    })
}

/// Look up a user by email, hashed email, or legacy profile ID
/// Returns profile info including legacy IDs for API compat
pub async fn lookup_profile_id(req: &Request) -> Result<ProfileIdLookupResponse, anyhow::Error> {
    let request: ProfileIdLookupRequest = decode_structured_message(req.message.clone())?;

    // First resolve to a user_id
    let user_id = match request.identifier_type {
        ProfileIdLookupType::Email => {
            sqlx::query_scalar!(
                r#"SELECT u.user_id
                   FROM emails e
                   JOIN users u ON e.user_id = u.user_id
                   WHERE e.email = ?
                   LIMIT 1"#,
                request.identifier
            )
            .fetch_optional(db())
            .await?
        }
        ProfileIdLookupType::Hashed => {
            sqlx::query_scalar!(
                r#"SELECT u.user_id
                   FROM emails e
                   JOIN users u ON e.user_id = u.user_id
                   WHERE e.hash = ? OR e.hash_lower = ? OR e.hash_capfirst = ?
                   LIMIT 1"#,
                request.identifier,
                request.identifier,
                request.identifier
            )
            .fetch_optional(db())
            .await?
        }
        ProfileIdLookupType::LegacyProfileId => {
            let profile_id: i64 = request.identifier.parse().unwrap_or(0);
            sqlx::query_scalar!(
                r#"SELECT user_id
                   FROM users
                   WHERE legacy_v3_profile_id = ? OR legacy_v2_profile_id = ?
                   LIMIT 1"#,
                profile_id,
                profile_id
            )
            .fetch_optional(db())
            .await?
        }
    };

    let Some(user_id) = user_id else {
        return Ok(ProfileIdLookupResponse {
            user_id: None,
            legacy_profile_id: None,
            opaque_id: None,
            known: false,
            track_contributions: true,
        });
    };

    // Fetch profile details
    let user = sqlx::query!(
        r#"SELECT legacy_v3_profile_id, opaque_id, track_contributions AS "track_contributions: bool"
           FROM users WHERE user_id = ?"#,
        user_id
    )
    .fetch_one(db())
    .await?;

    Ok(ProfileIdLookupResponse {
        user_id: Some(user_id),
        legacy_profile_id: user.legacy_v3_profile_id.map(|x| x as i64),
        opaque_id: user.opaque_id,
        known: true,
        track_contributions: user.track_contributions,
    })
}

/// Create a user account via API (for v1 /api/user/new compat)
pub async fn api_create_user(req: &Request) -> Result<ApiCreateUserResponse, anyhow::Error> {
    let request: ApiCreateUserRequest = decode_structured_message(req.message.clone())?;

    // Check if user already exists by email
    let existing = sqlx::query!(
        r#"SELECT u.user_id, u.legacy_v3_profile_id, u.opaque_id
           FROM emails e
           JOIN users u ON e.user_id = u.user_id
           WHERE e.email = ?
           LIMIT 1"#,
        request.email
    )
    .fetch_optional(db())
    .await?;

    if let Some(user) = existing {
        return Ok(ApiCreateUserResponse {
            user_id: user.user_id,
            legacy_profile_id: user.legacy_v3_profile_id.map(|x| x as i64),
            opaque_id: user.opaque_id,
            already_existed: true,
        });
    }

    // Create via NATS loopback to auth.signup
    use common::message::user::Signup;
    let signup = Signup {
        email: request.email.clone(),
        password: request.password.unwrap_or_default(),
        handle: request.handle.unwrap_or_default(),
        newsletter: true,
        zipcode: request.zipcode.unwrap_or_else(|| "00000".to_string()),
        protected: false,
        point: None,
        tags: vec![],
        disabled_password: false,
    };

    let basic_info: Option<common::message::user::BasicInfo> =
        common::service::request("user.auth.signup", signup).await?;

    let basic_info = basic_info.ok_or_else(|| anyhow::anyhow!("Failed to create user"))?;
    let user_id = basic_info.user_id.to_string();

    // Update first_name/last_name if provided
    if request.first_name.is_some() || request.last_name.is_some() {
        let first_name = request.first_name.unwrap_or_default();
        let last_name = request.last_name.unwrap_or_default();
        sqlx::query!(
            r#"UPDATE users SET first_name = ?, last_name = ? WHERE user_id = ?"#,
            first_name,
            last_name,
            user_id
        )
        .execute(db())
        .await?;
    }

    // Apply API-supplied account metadata (origin, referring project, user type).
    let origin = request.origin.unwrap_or_default();
    let referer = request.referer.unwrap_or_default();
    // Mirror orrery's misc format: {"user_type": N}. Empty when not supplied.
    let misc = request
        .user_type
        .map(|ut| format!(r#"{{"user_type": {}}}"#, ut))
        .unwrap_or_default();
    sqlx::query!(
        r#"UPDATE users SET origin = ?, referer = ?, misc = ? WHERE user_id = ?"#,
        origin,
        referer,
        misc,
        user_id
    )
    .execute(db())
    .await?;

    // Fetch the profile info
    let user = sqlx::query!(
        "SELECT legacy_v3_profile_id, opaque_id FROM users WHERE user_id = ?",
        user_id
    )
    .fetch_one(db())
    .await?;

    Ok(ApiCreateUserResponse {
        user_id,
        legacy_profile_id: user.legacy_v3_profile_id.map(|x| x as i64),
        opaque_id: user.opaque_id,
        already_existed: false,
    })
}

/// Validate an API key and return user info
pub async fn validate_api_key(req: &Request) -> Result<ValidateApiKeyResponse, anyhow::Error> {
    let request: ValidateApiKeyRequest = decode_structured_message(req.message.clone())?;

    let row = sqlx::query!(
        r#"SELECT user_id, is_staff, is_superuser,
            automatic_affiliate AS "automatic_affiliate: bool"
           FROM users WHERE api_key = ? LIMIT 1"#,
        request.api_key
    )
    .fetch_optional(db())
    .await?;

    if let Some(user) = row {
        Ok(ValidateApiKeyResponse {
            valid: true,
            user_id: Some(user.user_id),
            is_staff: user.is_staff != 0,
            is_superuser: user.is_superuser != 0,
            automatic_affiliate: user.automatic_affiliate,
        })
    } else {
        Ok(ValidateApiKeyResponse {
            valid: false,
            user_id: None,
            is_staff: false,
            is_superuser: false,
            automatic_affiliate: false,
        })
    }
}

/// List all users who opted out of AI recommendations
pub async fn ai_opted_out(req: &Request) -> Result<AiOptedOutResponse, anyhow::Error> {
    let _request: AiOptedOutRequest = decode_structured_message(req.message.clone())?;

    let rows = sqlx::query!(
        "SELECT opaque_id, ai_opt_out_at FROM users WHERE ai_opt_out = 1 AND opaque_id IS NOT NULL"
    )
    .fetch_all(db())
    .await?;

    Ok(AiOptedOutResponse {
        opted_out: rows
            .into_iter()
            .filter_map(|row| {
                Some(AiOptedOutEntry {
                    opaque_id: row.opaque_id?,
                    opted_out_at: row.ai_opt_out_at,
                })
            })
            .collect(),
    })
}
