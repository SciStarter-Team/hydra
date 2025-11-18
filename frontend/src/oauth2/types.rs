// OAuth2 / OIDC types and database structures

use anyhow::Error;
use common::{shared::db, timestring::TimeString};
use serde::{Deserialize, Serialize};
use crate::peering::LoadStorePeered;

/// OAuth2 Client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Client {
    pub client_id: String,
    pub client_secret: String,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    pub allowed_scopes: Vec<String>,
    pub created: TimeString,
    pub owner_user_id: Option<String>,
}

impl Client {
    /// Verify redirect URI is allowed for this client
    pub fn verify_redirect_uri(&self, uri: &str) -> bool {
        self.redirect_uris.iter().any(|allowed| allowed == uri)
    }

    /// Verify scope is allowed for this client
    pub fn verify_scope(&self, scope: &str) -> bool {
        let requested_scopes: Vec<&str> = scope.split_whitespace().collect();
        requested_scopes.iter().all(|s| self.allowed_scopes.contains(&s.to_string()))
    }

    /// Verify client secret
    pub fn verify_secret(&self, secret: &str) -> bool {
        self.client_secret == secret
    }
}

impl LoadStorePeered for Client {
    type DatabaseKey = String; // client_id
    const GROUP: &str = "oauth2_client";
    const SPECIFIC: &str = "client_id";

    async fn save_to_database(&self) -> Result<(), Error> {
        let redirect_uris = self.redirect_uris.join("\n");
        let allowed_scopes = self.allowed_scopes.join(" ");
        let created_str = self.created.to_string();

        sqlx::query!(
            "INSERT OR REPLACE INTO oauth2_clients
             (client_id, client_secret, client_name, redirect_uris, allowed_scopes, created, owner_user_id)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            self.client_id,
            self.client_secret,
            self.client_name,
            redirect_uris,
            allowed_scopes,
            created_str,
            self.owner_user_id
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(client_id: Self::DatabaseKey) -> Result<Self, Error> {
        let row = sqlx::query!(
            "SELECT client_id, client_secret, client_name, redirect_uris, allowed_scopes, created, owner_user_id
             FROM oauth2_clients WHERE client_id = ?",
            client_id
        )
        .fetch_one(db())
        .await?;

        Ok(Client {
            client_id: row.client_id,
            client_secret: row.client_secret,
            client_name: row.client_name,
            redirect_uris: row.redirect_uris.lines().map(|s| s.to_string()).collect(),
            allowed_scopes: row.allowed_scopes.split_whitespace().map(|s| s.to_string()).collect(),
            created: row.created.try_into()?,
            owner_user_id: row.owner_user_id,
        })
    }
}

/// Authorization Code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationCode {
    pub code: String,
    pub client_id: String,
    pub user_id: String,
    pub redirect_uri: String,
    pub scope: String,
    pub expires_at: TimeString,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub created: TimeString,
}

impl AuthorizationCode {
    /// Create a new authorization code
    pub fn new(
        client_id: String,
        user_id: String,
        redirect_uri: String,
        scope: String,
        code_challenge: Option<String>,
        code_challenge_method: Option<String>,
    ) -> Self {
        use rand::Rng;

        // Generate random code
        let code = {
            use base64::engine::Engine;
            use base64::engine::general_purpose::URL_SAFE_NO_PAD;
            let mut rng = rand::thread_rng();
            let bytes: Vec<u8> = (0..32).map(|_| rng.r#gen::<u8>()).collect();
            URL_SAFE_NO_PAD.encode(&bytes)
        };

        let created = TimeString::now();
        let expires_at = {
            use jiff::{Timestamp, ToSpan};
            let ts: Timestamp = created.clone().try_into().unwrap();
            let expires_ts = ts.checked_add(10.minutes()).unwrap();
            TimeString::from(expires_ts)
        };

        Self {
            code,
            client_id,
            user_id,
            redirect_uri,
            scope,
            expires_at,
            code_challenge,
            code_challenge_method,
            created,
        }
    }

    /// Check if code has expired
    pub fn is_expired(&self) -> bool {
        use jiff::Timestamp;
        let now: Timestamp = TimeString::now().try_into().unwrap();
        let expires: Timestamp = self.expires_at.clone().try_into().unwrap();
        now > expires
    }
}

impl LoadStorePeered for AuthorizationCode {
    type DatabaseKey = String; // code
    const GROUP: &str = "oauth2_authz";
    const SPECIFIC: &str = "code";

    async fn save_to_database(&self) -> Result<(), Error> {
        let expires_str = self.expires_at.to_string();
        let created_str = self.created.to_string();

        sqlx::query!(
            "INSERT OR REPLACE INTO oauth2_authorization_codes
             (code, client_id, user_id, redirect_uri, scope, expires_at, code_challenge, code_challenge_method, created)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            self.code,
            self.client_id,
            self.user_id,
            self.redirect_uri,
            self.scope,
            expires_str,
            self.code_challenge,
            self.code_challenge_method,
            created_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(code: Self::DatabaseKey) -> Result<Self, Error> {
        let row = sqlx::query!(
            "SELECT code, client_id, user_id, redirect_uri, scope, expires_at, code_challenge, code_challenge_method, created
             FROM oauth2_authorization_codes WHERE code = ?",
            code
        )
        .fetch_one(db())
        .await?;

        Ok(Self {
            code: row.code,
            client_id: row.client_id,
            user_id: row.user_id,
            redirect_uri: row.redirect_uri,
            scope: row.scope,
            expires_at: row.expires_at.try_into()?,
            code_challenge: row.code_challenge,
            code_challenge_method: row.code_challenge_method,
            created: row.created.try_into()?,
        })
    }

    async fn delete_from_database(code: Self::DatabaseKey) -> Result<(), Error> {
        sqlx::query!("DELETE FROM oauth2_authorization_codes WHERE code = ?", code)
            .execute(db())
            .await?;
        Ok(())
    }
}

/// Access Token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessToken {
    pub token: String,
    pub client_id: String,
    pub user_id: String,
    pub scope: String,
    pub expires_at: TimeString,
    pub created: TimeString,
}

impl AccessToken {
    /// Create a new access token
    pub fn new(client_id: String, user_id: String, scope: String) -> Self {
        use rand::Rng;

        // Generate random token
        let token = {
            use base64::engine::Engine;
            use base64::engine::general_purpose::URL_SAFE_NO_PAD;
            let mut rng = rand::thread_rng();
            let bytes: Vec<u8> = (0..32).map(|_| rng.r#gen::<u8>()).collect();
            URL_SAFE_NO_PAD.encode(&bytes)
        };

        let created = TimeString::now();
        let expires_at = {
            use jiff::{Timestamp, ToSpan};
            let ts: Timestamp = created.clone().try_into().unwrap();
            let expires_ts = ts.checked_add(1.hour()).unwrap();
            TimeString::from(expires_ts)
        };

        Self {
            token,
            client_id,
            user_id,
            scope,
            expires_at,
            created,
        }
    }

    /// Check if token has expired
    pub fn is_expired(&self) -> bool {
        use jiff::Timestamp;
        let now: Timestamp = TimeString::now().try_into().unwrap();
        let expires: Timestamp = self.expires_at.clone().try_into().unwrap();
        now > expires
    }
}

impl LoadStorePeered for AccessToken {
    type DatabaseKey = String; // token
    const GROUP: &str = "oauth2_token";
    const SPECIFIC: &str = "token";

    async fn save_to_database(&self) -> Result<(), Error> {
        let expires_str = self.expires_at.to_string();
        let created_str = self.created.to_string();

        sqlx::query!(
            "INSERT OR REPLACE INTO oauth2_access_tokens (token, client_id, user_id, scope, expires_at, created)
             VALUES (?, ?, ?, ?, ?, ?)",
            self.token,
            self.client_id,
            self.user_id,
            self.scope,
            expires_str,
            created_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(token: Self::DatabaseKey) -> Result<Self, Error> {
        let row = sqlx::query!(
            "SELECT token, client_id, user_id, scope, expires_at, created
             FROM oauth2_access_tokens WHERE token = ?",
            token
        )
        .fetch_one(db())
        .await?;

        Ok(Self {
            token: row.token,
            client_id: row.client_id,
            user_id: row.user_id,
            scope: row.scope,
            expires_at: row.expires_at.try_into()?,
            created: row.created.try_into()?,
        })
    }

    async fn delete_from_database(token: Self::DatabaseKey) -> Result<(), Error> {
        sqlx::query!("DELETE FROM oauth2_access_tokens WHERE token = ?", token)
            .execute(db())
            .await?;
        Ok(())
    }
}

/// Refresh Token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshToken {
    pub token: String,
    pub access_token: String,
    pub client_id: String,
    pub user_id: String,
    pub scope: String,
    pub expires_at: TimeString,
    pub created: TimeString,
}

impl RefreshToken {
    /// Create a new refresh token
    pub fn new(access_token: String, client_id: String, user_id: String, scope: String) -> Self {
        use rand::Rng;

        // Generate random token
        let token = {
            use base64::engine::Engine;
            use base64::engine::general_purpose::URL_SAFE_NO_PAD;
            let mut rng = rand::thread_rng();
            let bytes: Vec<u8> = (0..32).map(|_| rng.r#gen::<u8>()).collect();
            URL_SAFE_NO_PAD.encode(&bytes)
        };

        let created = TimeString::now();
        let expires_at = {
            use jiff::{Timestamp, ToSpan};
            let ts: Timestamp = created.clone().try_into().unwrap();
            let expires_ts = ts.checked_add(30.days()).unwrap();
            TimeString::from(expires_ts)
        };

        Self {
            token,
            access_token,
            client_id,
            user_id,
            scope,
            expires_at,
            created,
        }
    }

    /// Check if token has expired
    pub fn is_expired(&self) -> bool {
        use jiff::Timestamp;
        let now: Timestamp = TimeString::now().try_into().unwrap();
        let expires: Timestamp = self.expires_at.clone().try_into().unwrap();
        now > expires
    }
}

impl LoadStorePeered for RefreshToken {
    type DatabaseKey = String; // token
    const GROUP: &str = "oauth2_refresh";
    const SPECIFIC: &str = "token";

    async fn save_to_database(&self) -> Result<(), Error> {
        let expires_str = self.expires_at.to_string();
        let created_str = self.created.to_string();

        sqlx::query!(
            "INSERT OR REPLACE INTO oauth2_refresh_tokens (token, access_token, client_id, user_id, scope, expires_at, created)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            self.token,
            self.access_token,
            self.client_id,
            self.user_id,
            self.scope,
            expires_str,
            created_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(token: Self::DatabaseKey) -> Result<Self, Error> {
        let row = sqlx::query!(
            "SELECT token, access_token, client_id, user_id, scope, expires_at, created
             FROM oauth2_refresh_tokens WHERE token = ?",
            token
        )
        .fetch_one(db())
        .await?;

        Ok(Self {
            token: row.token,
            access_token: row.access_token,
            client_id: row.client_id,
            user_id: row.user_id,
            scope: row.scope,
            expires_at: row.expires_at.try_into()?,
            created: row.created.try_into()?,
        })
    }

    async fn delete_from_database(token: Self::DatabaseKey) -> Result<(), Error> {
        sqlx::query!("DELETE FROM oauth2_refresh_tokens WHERE token = ?", token)
            .execute(db())
            .await?;
        Ok(())
    }
}
