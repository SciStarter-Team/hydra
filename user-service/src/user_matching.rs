// User matching for API participation recording
// Returns None when user doesn't exist (API caller handles this by returning success/discard)

use async_nats::service::Request;
use common::{service::decode_structured_message, shared::db};
use common::message::user::{IdentifierType, UserMatchRequest, UserMatchResponse, UserApiKeyRequest, UserApiKeyResponse, UserOpaqueIdRequest, UserOpaqueIdResponse, ValidateApiKeyRequest, ValidateApiKeyResponse};

/// Match user by identifier (hashed email, opaque ID, email, or link token)
pub async fn match_user(req: &Request) -> Result<UserMatchResponse, anyhow::Error> {
    let request: UserMatchRequest = decode_structured_message(req.message.clone())?;

    let user_id = match request.identifier_type {
        IdentifierType::Hashed => match_by_hashed_email(&request.identifier).await?,
        IdentifierType::Opaque => match_by_opaque_id(&request.identifier).await?,
        IdentifierType::Email => match_by_email(&request.identifier).await?,
        IdentifierType::LinkToken => match_by_link_token(&request.identifier).await?,
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
    let row = sqlx::query!(
        "SELECT user_id FROM emails WHERE email = ? LIMIT 1",
        email
    )
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

/// Validate an API key and return user info
pub async fn validate_api_key(req: &Request) -> Result<ValidateApiKeyResponse, anyhow::Error> {
    let request: ValidateApiKeyRequest = decode_structured_message(req.message.clone())?;

    let row = sqlx::query!(
        "SELECT user_id, is_staff, is_superuser FROM users WHERE api_key = ? LIMIT 1",
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
        })
    } else {
        Ok(ValidateApiKeyResponse {
            valid: false,
            user_id: None,
            is_staff: false,
            is_superuser: false,
        })
    }
}
