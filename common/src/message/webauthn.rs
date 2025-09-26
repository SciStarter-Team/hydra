use super::user::BasicInfo;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// Request to start WebAuthn credential registration for a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnRegistrationStartRequest {
    pub user_id: Ulid,
    pub username: String,
    pub display_name: String,
}

/// Response containing the WebAuthn credential creation challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnRegistrationStartResponse {
    /// The credential creation options JSON that should be sent to the browser
    pub creation_options_json: String,
    /// Internal registration state that must be stored server-side for verification
    pub registration_state_json: String,
}

/// Request to complete WebAuthn credential registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnRegistrationCompleteRequest {
    pub user_id: Ulid,
    /// The credential creation response JSON from the browser
    pub credential_json: String,
    /// The registration state from the start request (stored server-side)
    pub registration_state_json: String,
}

/// Response after completing WebAuthn credential registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnRegistrationCompleteResponse {
    /// Unique ID for this credential (for later reference)
    pub credential_id: Vec<u8>,
    /// Success/failure status
    pub success: bool,
    /// Error message if registration failed
    pub error_message: Option<String>,
}

/// Request to start WebAuthn authentication for a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnAuthenticationStartRequest {
    /// Optional user email - if provided, authentication is for this specific user
    /// If not provided, will use discoverable credentials (usernameless flow)
    pub email: Option<String>,
}

/// Response containing the WebAuthn authentication challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnAuthenticationStartResponse {
    /// The credential request options JSON that should be sent to the browser
    pub request_options_json: String,
    /// Internal authentication state that must be stored server-side for verification
    pub authentication_state_json: String,
}

/// Request to complete WebAuthn authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnAuthenticationCompleteRequest {
    /// The authentication response JSON from the browser
    pub credential_json: String,
    /// The authentication state from the start request (stored server-side)
    pub authentication_state_json: String,
}

/// Response after completing WebAuthn authentication
/// Returns BasicInfo just like regular login for seamless integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnAuthenticationCompleteResponse {
    /// The authenticated user's basic info (None if authentication failed)
    /// This mirrors the response from user.auth.login for consistency
    pub basic_info: Option<BasicInfo>,
    /// Error message if authentication failed
    pub error_message: Option<String>,
}

/// Request to list a user's WebAuthn credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnCredentialsListRequest {
    pub user_id: Ulid,
}

/// Information about a single WebAuthn credential
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnCredentialInfo {
    pub credential_id: Vec<u8>,
    pub display_name: Option<String>,
    pub created_at: String,        // ISO 8601 timestamp
    pub last_used: Option<String>, // ISO 8601 timestamp
}

/// Response containing a user's WebAuthn credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnCredentialsListResponse {
    pub credentials: Vec<WebAuthnCredentialInfo>,
}

/// Request to delete a WebAuthn credential
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnCredentialDeleteRequest {
    pub user_id: Ulid,
    pub credential_id: Vec<u8>,
}

/// Response after deleting a WebAuthn credential
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnCredentialDeleteResponse {
    pub success: bool,
    pub error_message: Option<String>,
}
