// OAuth2 UserInfo Endpoint

use poem::{handler, Response, IntoResponse, http::HeaderMap};
use anyhow::Error;
use serde::{Serialize, Deserialize};
use serde_json::json;
use crate::oauth2::types::AccessToken;
use crate::peering::LoadStorePeered;
use common::service;

#[derive(Debug, Serialize)]
struct UserinfoErrorResponse {
    error: String,
    error_description: String,
}

/// GET/POST /oauth2/userinfo
/// Returns user information based on access token
#[handler]
pub async fn userinfo_endpoint(headers: &HeaderMap) -> Result<Response, Error> {
    // Extract bearer token from Authorization header
    let token = match extract_bearer_token(headers) {
        Some(t) => t,
        None => {
            return Ok(error_response(
                "invalid_request",
                "Missing or invalid Authorization header",
                poem::http::StatusCode::UNAUTHORIZED,
            ));
        }
    };

    // Load and validate access token
    let access_token = match AccessToken::load(token.to_string()).await {
        Ok(at) => at,
        Err(_) => {
            return Ok(error_response(
                "invalid_token",
                "Invalid or expired access token",
                poem::http::StatusCode::UNAUTHORIZED,
            ));
        }
    };

    // Check if token has expired
    if access_token.is_expired() {
        AccessToken::delete(access_token.token.clone()).await?;
        return Ok(error_response(
            "invalid_token",
            "Access token has expired",
            poem::http::StatusCode::UNAUTHORIZED,
        ));
    }

    // Fetch user info from user-service
    #[derive(Serialize, Deserialize)]
    struct UserInfoRequest {
        user_id: String,
    }

    #[derive(Deserialize, Serialize)]
    struct UserInfoFromService {
        user_id: String,
        handle: String,
        emails: Vec<String>,
    }

    let user_info: UserInfoFromService = service::request(
        "user.info.extended",
        UserInfoRequest {
            user_id: access_token.user_id.clone(),
        },
    )
    .await?;

    // Build response based on granted scopes
    let scopes: Vec<&str> = access_token.scope.split_whitespace().collect();

    // Base response with sub (always included)
    let mut response_json = json!({
        "sub": access_token.user_id.clone(),
    });

    // Add email if email scope granted
    if scopes.contains(&"email") || scopes.contains(&"openid") {
        if let Some(email) = user_info.emails.first() {
            response_json["email"] = json!(email);
        }
    }

    // Add profile info if profile scope granted
    if scopes.contains(&"profile") || scopes.contains(&"openid") {
        response_json["name"] = json!(user_info.handle.clone());
        response_json["handle"] = json!(user_info.handle);
    }

    // Add participation permission grant if participation scope granted
    if scopes.contains(&"participation") {
        response_json["participation_api_granted"] = json!(true);
    }

    Ok(poem::web::Json(response_json).into_response())
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| {
            if auth.starts_with("Bearer ") {
                Some(&auth[7..])
            } else {
                None
            }
        })
}

fn error_response(error: &str, description: &str, status: poem::http::StatusCode) -> Response {
    let err = UserinfoErrorResponse {
        error: error.to_string(),
        error_description: description.to_string(),
    };
    poem::web::Json(err)
        .with_status(status)
        .with_header("WWW-Authenticate", "Bearer")
        .into_response()
}
