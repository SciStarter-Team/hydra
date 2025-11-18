// OAuth2 Token Endpoint

use poem::{handler, Response, IntoResponse, web::Form};
use anyhow::Error;
use serde::{Deserialize, Serialize};
use crate::oauth2::types::{Client, AuthorizationCode, AccessToken, RefreshToken};
use crate::peering::LoadStorePeered;

#[derive(Debug, Deserialize)]
pub struct TokenRequest {
    grant_type: String,
    code: Option<String>,
    redirect_uri: Option<String>,
    client_id: String,
    client_secret: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id_token: Option<String>,
    scope: String,
}

#[derive(Debug, Serialize)]
pub struct TokenErrorResponse {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_description: Option<String>,
}

/// POST /oauth2/token
/// Exchanges authorization code for access token
#[handler]
pub async fn token_endpoint(
    Form(req): Form<TokenRequest>,
) -> Result<Response, Error> {
    match req.grant_type.as_str() {
        "authorization_code" => handle_authorization_code_grant(req).await,
        "refresh_token" => handle_refresh_token_grant(req).await,
        _ => Ok(error_response("unsupported_grant_type", None)),
    }
}

async fn handle_authorization_code_grant(req: TokenRequest) -> Result<Response, Error> {
    // Validate required parameters
    let code = match req.code {
        Some(c) => c,
        None => return Ok(error_response("invalid_request", Some("Missing code parameter"))),
    };

    let redirect_uri = match req.redirect_uri {
        Some(uri) => uri,
        None => return Ok(error_response("invalid_request", Some("Missing redirect_uri parameter"))),
    };

    // Load client
    let client = match Client::load(req.client_id.clone()).await {
        Ok(client) => client,
        Err(_) => return Ok(error_response("invalid_client", Some("Unknown client_id"))),
    };

    // Verify client_secret
    if let Some(secret) = req.client_secret {
        if client.client_secret != secret {
            return Ok(error_response("invalid_client", Some("Invalid client_secret")));
        }
    } else {
        return Ok(error_response("invalid_request", Some("Missing client_secret")));
    }

    // Load authorization code
    let auth_code = match AuthorizationCode::load(code.clone()).await {
        Ok(ac) => ac,
        Err(_) => return Ok(error_response("invalid_grant", Some("Invalid or expired authorization code"))),
    };

    // Validate authorization code
    if auth_code.client_id != req.client_id {
        return Ok(error_response("invalid_grant", Some("Code was issued to different client")));
    }

    if auth_code.redirect_uri != redirect_uri {
        return Ok(error_response("invalid_grant", Some("redirect_uri mismatch")));
    }

    // Check if code has expired
    let now: jiff::Timestamp = jiff::Timestamp::now();
    let expires_at: jiff::Timestamp = auth_code.expires_at.clone().try_into()
        .map_err(|e| anyhow::anyhow!("Failed to parse expires_at: {}", e))?;

    if now > expires_at {
        AuthorizationCode::delete(code.clone()).await?;
        return Ok(error_response("invalid_grant", Some("Authorization code has expired")));
    }

    // Verify PKCE if code_challenge was used
    if let Some(challenge) = &auth_code.code_challenge {
        let verifier = match req.code_verifier {
            Some(v) => v,
            None => return Ok(error_response("invalid_request", Some("code_verifier required"))),
        };

        let method = auth_code.code_challenge_method.as_deref().unwrap_or("plain");
        if !verify_pkce(challenge, &verifier, method) {
            return Ok(error_response("invalid_grant", Some("PKCE verification failed")));
        }
    }

    // Generate access token
    let access_token = AccessToken::new(
        auth_code.client_id.clone(),
        auth_code.user_id.clone(),
        auth_code.scope.clone(),
    );

    // Generate refresh token
    let refresh_token = RefreshToken::new(
        access_token.token.clone(),
        auth_code.client_id.clone(),
        auth_code.user_id.clone(),
    );

    // Store tokens via LoadStorePeered
    access_token.store().await?;
    refresh_token.store().await?;

    // Delete one-time use authorization code
    AuthorizationCode::delete(code.clone()).await?;

    // Generate ID token if openid scope is requested
    let id_token = if auth_code.scope.contains("openid") {
        Some(generate_id_token(&auth_code.user_id, &req.client_id)?)
    } else {
        None
    };

    // Calculate expires_in
    let access_expires_at: jiff::Timestamp = access_token.expires_at.clone().try_into()?;
    let expires_in = (access_expires_at - now).total(jiff::Unit::Second)? as i64;

    let response = TokenResponse {
        access_token: access_token.token,
        token_type: "Bearer".to_string(),
        expires_in,
        refresh_token: Some(refresh_token.token),
        id_token,
        scope: auth_code.scope,
    };

    Ok(poem::web::Json(response).into_response())
}

async fn handle_refresh_token_grant(req: TokenRequest) -> Result<Response, Error> {
    // Validate required parameters
    let refresh_token_str = match req.refresh_token {
        Some(t) => t,
        None => return Ok(error_response("invalid_request", Some("Missing refresh_token parameter"))),
    };

    // Load client
    let client = match Client::load(req.client_id.clone()).await {
        Ok(client) => client,
        Err(_) => return Ok(error_response("invalid_client", Some("Unknown client_id"))),
    };

    // Verify client_secret
    if let Some(secret) = req.client_secret {
        if client.client_secret != secret {
            return Ok(error_response("invalid_client", Some("Invalid client_secret")));
        }
    } else {
        return Ok(error_response("invalid_request", Some("Missing client_secret")));
    }

    // Load refresh token
    let old_refresh = match RefreshToken::load(refresh_token_str.clone()).await {
        Ok(rt) => rt,
        Err(_) => return Ok(error_response("invalid_grant", Some("Invalid refresh token"))),
    };

    // Validate refresh token
    if old_refresh.client_id != req.client_id {
        return Ok(error_response("invalid_grant", Some("Token was issued to different client")));
    }

    // Check if refresh token has expired
    let now: jiff::Timestamp = jiff::Timestamp::now();
    let expires_at: jiff::Timestamp = old_refresh.expires_at.clone().try_into()
        .map_err(|e| anyhow::anyhow!("Failed to parse expires_at: {}", e))?;

    if now > expires_at {
        RefreshToken::delete(refresh_token_str.clone()).await?;
        return Ok(error_response("invalid_grant", Some("Refresh token has expired")));
    }

    // Delete old access token
    AccessToken::delete(old_refresh.access_token.clone()).await?;

    // Generate new access token (use empty scope for now, could store scope in refresh token table)
    let access_token = AccessToken::new(
        old_refresh.client_id.clone(),
        old_refresh.user_id.clone(),
        "openid".to_string(), // TODO: Store original scope in refresh token
    );

    // Store new access token
    access_token.store().await?;

    // Update refresh token to point to new access token
    let new_refresh = RefreshToken {
        access_token: access_token.token.clone(),
        ..old_refresh
    };
    new_refresh.store().await?;

    // Calculate expires_in
    let access_expires_at: jiff::Timestamp = access_token.expires_at.clone().try_into()?;
    let expires_in = (access_expires_at - now).total(jiff::Unit::Second)? as i64;

    let response = TokenResponse {
        access_token: access_token.token,
        token_type: "Bearer".to_string(),
        expires_in,
        refresh_token: Some(refresh_token_str),
        id_token: None,
        scope: "openid".to_string(),
    };

    Ok(poem::web::Json(response).into_response())
}

fn verify_pkce(challenge: &str, verifier: &str, method: &str) -> bool {
    match method {
        "plain" => challenge == verifier,
        "S256" => {
            use sha2::{Sha256, Digest};
            use base64::engine::Engine;
            use base64::engine::general_purpose::URL_SAFE_NO_PAD;

            let hash = Sha256::digest(verifier.as_bytes());
            let computed_challenge = URL_SAFE_NO_PAD.encode(&hash);
            challenge == computed_challenge
        }
        _ => false,
    }
}

fn generate_id_token(user_id: &str, client_id: &str) -> Result<String, Error> {
    use jsonwebtoken::{encode, EncodingKey, Header, Algorithm};
    use serde::Serialize;

    #[derive(Serialize)]
    struct IdTokenClaims {
        iss: String,
        sub: String,
        aud: String,
        exp: i64,
        iat: i64,
    }

    let now = jiff::Timestamp::now();
    let exp = now.checked_add(jiff::ToSpan::hours(1))?;

    let claims = IdTokenClaims {
        iss: format!("https://{}", env!("DOMAIN")),
        sub: user_id.to_string(),
        aud: client_id.to_string(),
        exp: exp.as_second(),
        iat: now.as_second(),
    };

    // Use RSA private key for signing
    use rsa::pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey};
    use rsa::RsaPrivateKey;

    let pem = env!("OIDC_RSA_PRIVATE_KEY");
    let private_key = RsaPrivateKey::from_pkcs1_pem(pem)?;
    let der = private_key.to_pkcs1_der()?;

    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("scistarter-oidc-key".to_string());

    let token = encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_der(der.as_bytes()),
    )?;

    Ok(token)
}

fn error_response(error: &str, description: Option<&str>) -> Response {
    let err = TokenErrorResponse {
        error: error.to_string(),
        error_description: description.map(|s| s.to_string()),
    };
    poem::web::Json(err).with_status(poem::http::StatusCode::BAD_REQUEST).into_response()
}
