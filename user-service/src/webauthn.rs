use anyhow::{Context, Result, anyhow};
use async_nats::service::Request;
use common::{
    message::{
        user::BasicInfo,
        webauthn::{
            WebAuthnAuthenticationCompleteRequest, WebAuthnAuthenticationCompleteResponse,
            WebAuthnAuthenticationStartRequest, WebAuthnAuthenticationStartResponse,
            WebAuthnCredentialDeleteRequest, WebAuthnCredentialDeleteResponse,
            WebAuthnCredentialInfo, WebAuthnCredentialsListRequest,
            WebAuthnCredentialsListResponse, WebAuthnRegistrationCompleteRequest,
            WebAuthnRegistrationCompleteResponse, WebAuthnRegistrationStartRequest,
            WebAuthnRegistrationStartResponse,
        },
    },
    shared::db,
    webauthn::ulid_to_uuid,
};
use flexbuffers;
use serde_json;
use ulid::Ulid;
use url::Url;
use webauthn_rs::prelude::*;

/// Initialize the WebAuthn instance for this service
fn init_webauthn() -> Result<Webauthn> {
    // Use localhost configuration in debug mode for development
    let (rp_id, rp_origin) = if cfg!(debug_assertions) {
        ("localhost", Url::parse("http://localhost:2222").unwrap())
    } else {
        let rp_id = env!("WEBAUTHN_ID");
        let rp_origin =
            Url::parse(env!("WEBAUTHN_ORIGIN")).context("Invalid WEBAUTHN_ORIGIN URL")?;
        (rp_id, rp_origin)
    };

    let mut builder =
        WebauthnBuilder::new(rp_id, &rp_origin).context("Failed to create WebAuthn builder")?;

    // Allow subdomains (this enables both www.example.com and example.com)
    builder = builder.allow_subdomains(true);

    let webauthn = builder
        .build()
        .context("Failed to build WebAuthn instance")?;

    Ok(webauthn)
}

/// Start WebAuthn credential registration for a user
pub async fn registration_start(request: &Request) -> Result<WebAuthnRegistrationStartResponse> {
    let req: WebAuthnRegistrationStartRequest = flexbuffers::from_slice(&request.message.payload)?;

    let webauthn = init_webauthn()?;

    // Convert ULID to UUID for webauthn-rs
    let user_uuid = ulid_to_uuid(req.user_id);

    // Get existing credentials for this user to exclude them
    let user_id_str = req.user_id.to_string();
    let existing_creds = sqlx::query!(
        "SELECT credential_id FROM webauthn_credentials WHERE user_id = ?",
        user_id_str
    )
    .fetch_all(db())
    .await
    .context("Failed to fetch existing credentials")?;

    let exclude_credentials = if existing_creds.is_empty() {
        None
    } else {
        Some(
            existing_creds
                .into_iter()
                .filter_map(|row| row.credential_id)
                .map(|bytes| CredentialID::from(bytes))
                .collect(),
        )
    };

    // Start passkey registration
    let (ccr, skr) = webauthn
        .start_passkey_registration(
            user_uuid,
            &req.username,
            &req.display_name,
            exclude_credentials,
        )
        .context("Failed to start passkey registration")?;

    // Serialize the challenge response and state
    let creation_options_json =
        serde_json::to_string(&ccr).context("Failed to serialize creation challenge response")?;

    // Store the registration state in database as blob
    let session_id = ulid::Ulid::new().to_string();
    let state_data = flexbuffers::to_vec(&skr)?;
    let user_id_str = req.user_id.to_string();

    sqlx::query!(
        "INSERT INTO webauthn_sessions (session_id, session_type, user_id, state_data) VALUES (?, ?, ?, ?)",
        session_id,
        "registration",
        user_id_str,
        state_data
    )
    .execute(db())
    .await
    .context("Failed to store registration session")?;

    let registration_state_json = session_id;

    Ok(WebAuthnRegistrationStartResponse {
        creation_options_json,
        registration_state_json,
    })
}

/// Complete WebAuthn credential registration for a user
pub async fn registration_complete(
    request: &Request,
) -> Result<WebAuthnRegistrationCompleteResponse> {
    let req: WebAuthnRegistrationCompleteRequest =
        flexbuffers::from_slice(&request.message.payload)?;

    let webauthn = init_webauthn()?;

    // Retrieve the registration state from database
    let session_data = sqlx::query!(
        "SELECT state_data FROM webauthn_sessions WHERE session_id = ? AND session_type = ? AND expires_at > datetime('now')",
        req.registration_state_json,
        "registration"
    )
    .fetch_optional(db())
    .await
    .context("Failed to fetch registration session")?;

    let state_data = session_data
        .ok_or_else(|| anyhow!("Registration session not found or expired"))?
        .state_data;

    let registration_state: PasskeyRegistration =
        flexbuffers::from_slice(&state_data).context("Failed to deserialize registration state")?;

    // Clean up the session after use
    sqlx::query!(
        "DELETE FROM webauthn_sessions WHERE session_id = ?",
        req.registration_state_json
    )
    .execute(db())
    .await
    .context("Failed to cleanup registration session")?;

    let credential_response: RegisterPublicKeyCredential =
        serde_json::from_str(&req.credential_json)
            .context("Failed to deserialize credential response")?;

    // Complete the registration
    let passkey = webauthn
        .finish_passkey_registration(&credential_response, &registration_state)
        .context("Failed to complete passkey registration")?;

    // Store the credential in the database
    let user_id_str = req.user_id.to_string();
    let credential_id = passkey.cred_id().as_slice().to_vec();
    let credential_data = serde_json::to_string(&passkey).context("Failed to serialize passkey")?;

    sqlx::query!(
        r#"INSERT INTO webauthn_credentials (credential_id, user_id, credential_data, created_at)
           VALUES (?, ?, ?, datetime('now'))"#,
        credential_id,
        user_id_str,
        credential_data,
    )
    .execute(db())
    .await
    .context("Failed to store WebAuthn credential")?;

    Ok(WebAuthnRegistrationCompleteResponse {
        credential_id,
        success: true,
        error_message: None,
    })
}

/// Start WebAuthn authentication
pub async fn authentication_start(
    request: &Request,
) -> Result<WebAuthnAuthenticationStartResponse> {
    let req: WebAuthnAuthenticationStartRequest =
        flexbuffers::from_slice(&request.message.payload)?;

    let webauthn = init_webauthn()?;

    // Get user credentials for authentication
    let credentials = if let Some(ref email) = req.email {
        // User-specific authentication - find user by email and get their credentials
        let user_credentials = sqlx::query!(
            r#"SELECT wc.credential_data
               FROM users u
               JOIN emails e ON u.user_id = e.user_id
               JOIN webauthn_credentials wc ON u.user_id = wc.user_id
               WHERE e.email = ? AND e.verified = 1"#,
            email
        )
        .fetch_all(db())
        .await
        .context("Failed to fetch user credentials")?;

        if user_credentials.is_empty() {
            return Err(anyhow!(
                "No verified user found with that email or no credentials"
            ));
        }

        user_credentials
            .into_iter()
            .filter_map(|row| serde_json::from_str::<Passkey>(&row.credential_data).ok())
            .collect::<Vec<Passkey>>()
    } else {
        // Usernameless/discoverable credential authentication
        // Don't fetch any credentials - just provide an empty list
        // The browser will handle credential discovery and provide the credential ID
        // in the authentication response
        Vec::new()
    };

    // Start passkey authentication
    let (rcr, skr) = webauthn
        .start_passkey_authentication(&credentials)
        .context("Failed to start passkey authentication")?;

    // Serialize the challenge response and state
    let request_options_json = serde_json::to_string(&rcr)
        .context("Failed to serialize authentication challenge response")?;

    // Store the authentication state in database as blob
    let session_id = ulid::Ulid::new().to_string();
    let state_data = flexbuffers::to_vec(&skr)?;

    sqlx::query!(
        "INSERT INTO webauthn_sessions (session_id, session_type, user_id, state_data) VALUES (?, ?, ?, ?)",
        session_id,
        "authentication",
        req.email, // Store email for user-specific auth, NULL for usernameless
        state_data
    )
    .execute(db())
    .await
    .context("Failed to store authentication session")?;

    let authentication_state_json = session_id;

    Ok(WebAuthnAuthenticationStartResponse {
        request_options_json,
        authentication_state_json,
    })
}

/// Complete WebAuthn authentication
pub async fn authentication_complete(
    request: &Request,
) -> Result<WebAuthnAuthenticationCompleteResponse> {
    let req: WebAuthnAuthenticationCompleteRequest =
        flexbuffers::from_slice(&request.message.payload)?;

    let webauthn = init_webauthn()?;

    // Retrieve the authentication state from database
    let session_data = sqlx::query!(
        "SELECT state_data FROM webauthn_sessions WHERE session_id = ? AND session_type = ? AND expires_at > datetime('now')",
        req.authentication_state_json,
        "authentication"
    )
    .fetch_optional(db())
    .await
    .context("Failed to fetch authentication session")?;

    let state_data = session_data
        .ok_or_else(|| anyhow!("Authentication session not found or expired"))?
        .state_data;

    let authentication_state: PasskeyAuthentication = flexbuffers::from_slice(&state_data)
        .context("Failed to deserialize authentication state")?;

    // Clean up the session after use
    sqlx::query!(
        "DELETE FROM webauthn_sessions WHERE session_id = ?",
        req.authentication_state_json
    )
    .execute(db())
    .await
    .context("Failed to cleanup authentication session")?;

    let credential_response: PublicKeyCredential = serde_json::from_str(&req.credential_json)
        .context("Failed to deserialize credential response")?;

    // For usernameless auth, we need to look up the credential first
    // Extract the credential ID from the response to find the right credential
    let credential_id = credential_response.raw_id.as_slice();

    // Look up this specific credential in our database
    let credential_data = sqlx::query!(
        "SELECT credential_data FROM webauthn_credentials WHERE credential_id = ?",
        credential_id
    )
    .fetch_optional(db())
    .await
    .context("Failed to fetch credential for authentication")?;

    let passkey = if let Some(cred_row) = credential_data {
        serde_json::from_str::<Passkey>(&cred_row.credential_data)
            .context("Failed to deserialize stored passkey")?
    } else {
        return Ok(WebAuthnAuthenticationCompleteResponse {
            basic_info: None,
            error_message: Some("Credential not found".to_string()),
        });
    };

    // Complete the authentication with the specific credential
    let auth_result = webauthn
        .finish_passkey_authentication(&credential_response, &authentication_state)
        .context("Failed to complete passkey authentication")?;

    // Validate that the authenticated credential matches our stored credential
    if auth_result.cred_id() != passkey.cred_id() {
        return Ok(WebAuthnAuthenticationCompleteResponse {
            basic_info: None,
            error_message: Some("Credential mismatch during authentication".to_string()),
        });
    }

    // Update the stored credential with the new counter value to prevent replay attacks
    let updated_credential_data =
        serde_json::to_string(&auth_result).context("Failed to serialize updated credential")?;

    sqlx::query!(
        "UPDATE webauthn_credentials SET credential_data = ?, last_used = datetime('now') WHERE credential_id = ?",
        updated_credential_data,
        credential_id
    )
    .execute(db())
    .await
    .context("Failed to update credential after authentication")?;

    // Get the associated user
    let user_data = sqlx::query!(
        r#"SELECT u.user_id, u.handle, u.is_superuser, u.is_page_manager, u.public, u.protected, u.point
           FROM users u
           JOIN webauthn_credentials wc ON u.user_id = wc.user_id
           WHERE wc.credential_id = ?"#,
        credential_id
    )
    .fetch_optional(db())
    .await
    .context("Failed to fetch user for authenticated credential")?;

    if let Some(user) = user_data {
        // Convert back to ULID and create BasicInfo
        let user_id = user
            .user_id
            .parse::<Ulid>()
            .context("Invalid user ID in database")?;

        let basic_info = BasicInfo {
            user_id,
            handle: user.handle,
            is_superuser: user.is_superuser != 0,
            is_staff: user.is_superuser != 0 || user.is_page_manager != 0,
            public: user.public != 0,
            protected: user.protected != 0,
            needs_point: user.point.as_ref().map_or(true, |p| p.is_empty()),
        };

        Ok(WebAuthnAuthenticationCompleteResponse {
            basic_info: Some(basic_info),
            error_message: None,
        })
    } else {
        Ok(WebAuthnAuthenticationCompleteResponse {
            basic_info: None,
            error_message: Some("Authentication successful but user not found".to_string()),
        })
    }
}

/// List a user's WebAuthn credentials
pub async fn credentials_list(request: &Request) -> Result<WebAuthnCredentialsListResponse> {
    let req: WebAuthnCredentialsListRequest = flexbuffers::from_slice(&request.message.payload)?;

    let user_id_str = req.user_id.to_string();
    let credentials = sqlx::query!(
        "SELECT credential_id as \"credential_id!\", display_name, created_at as \"created_at!\", last_used FROM webauthn_credentials WHERE user_id = ? ORDER BY created_at DESC",
        user_id_str
    )
    .fetch_all(db())
    .await
    .context("Failed to fetch user credentials")?;

    let credential_infos = credentials
        .into_iter()
        .map(|row| WebAuthnCredentialInfo {
            credential_id: row.credential_id,
            display_name: row.display_name,
            created_at: row.created_at,
            last_used: row.last_used,
        })
        .collect();

    Ok(WebAuthnCredentialsListResponse {
        credentials: credential_infos,
    })
}

/// Delete a user's WebAuthn credential
pub async fn credential_delete(request: &Request) -> Result<WebAuthnCredentialDeleteResponse> {
    let req: WebAuthnCredentialDeleteRequest = flexbuffers::from_slice(&request.message.payload)?;

    let user_id_str = req.user_id.to_string();
    let result = sqlx::query!(
        "DELETE FROM webauthn_credentials WHERE user_id = ? AND credential_id = ?",
        user_id_str,
        req.credential_id
    )
    .execute(db())
    .await
    .context("Failed to delete WebAuthn credential")?;

    if result.rows_affected() > 0 {
        Ok(WebAuthnCredentialDeleteResponse {
            success: true,
            error_message: None,
        })
    } else {
        Ok(WebAuthnCredentialDeleteResponse {
            success: false,
            error_message: Some("Credential not found or access denied".to_string()),
        })
    }
}

/// Clean up expired WebAuthn sessions
pub async fn cleanup_expired_sessions() -> Result<()> {
    sqlx::query!("DELETE FROM webauthn_sessions WHERE expires_at <= datetime('now')")
        .execute(db())
        .await
        .context("Failed to cleanup expired WebAuthn sessions")?;

    Ok(())
}
