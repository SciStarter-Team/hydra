//! Bearer tokens authorizing access to the content-management MCP
//! endpoint (see [`crate::mcp::content`]).
//!
//! Tokens are issued and revoked through the admin Engine Room. They do
//! not expire; revoking a token means deleting it. Like the OAuth2
//! token types, [`McpToken`] is [`LoadStorePeered`] so that a token
//! issued on one frontend instance can be validated on any instance.

use anyhow::Error;
use common::{shared::db, timestring::TimeString};

use crate::peering::LoadStorePeered;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpToken {
    /// The secret bearer token. This is the primary key.
    pub token: String,
    /// A human-readable label identifying who or what the token is for.
    pub name: String,
    pub created: TimeString,
}

impl McpToken {
    /// Create a new token with a freshly generated secret.
    pub fn new(name: impl Into<String>) -> McpToken {
        McpToken {
            token: generate_token(),
            name: name.into(),
            created: TimeString::now(),
        }
    }

    /// Validate a presented bearer token, returning the matching record
    /// if it exists. Tokens never expire, so existence is validity.
    pub async fn validate(token: &str) -> Result<McpToken, Error> {
        McpToken::load(token.to_string()).await
    }

    /// List all tokens, newest first, for the admin interface.
    pub async fn list() -> Result<Vec<McpToken>, Error> {
        struct Row {
            token: String,
            name: String,
            created: String,
        }

        let rows = sqlx::query_as!(
            Row,
            r#"SELECT "token", "name", "created" FROM "mcp_tokens" ORDER BY "created" DESC"#
        )
        .fetch_all(db())
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(McpToken {
                    token: row.token,
                    name: row.name,
                    created: TimeString::try_from(row.created)?,
                })
            })
            .collect()
    }
}

/// Generate a random 256-bit token, encoded as URL-safe base64. This
/// mirrors the OAuth2 access token generation in
/// [`crate::oauth2::types`].
fn generate_token() -> String {
    use base64::engine::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use rand::Rng;

    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.r#gen::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(&bytes)
}

impl LoadStorePeered for McpToken {
    type DatabaseKey = String; // token

    const GROUP: &str = "mcp_token";

    async fn save_to_database(&self) -> Result<(), Error> {
        let created = self.created.to_string();

        sqlx::query!(
            r#"INSERT OR REPLACE INTO "mcp_tokens" ("token", "name", "created")
               VALUES (?, ?, ?)"#,
            self.token,
            self.name,
            created,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(token: Self::DatabaseKey) -> Result<Self, Error> {
        struct Row {
            token: String,
            name: String,
            created: String,
        }

        let row = sqlx::query_as!(
            Row,
            r#"SELECT "token", "name", "created" FROM "mcp_tokens" WHERE "token" = ?"#,
            token,
        )
        .fetch_one(db())
        .await?;

        Ok(McpToken {
            token: row.token,
            name: row.name,
            created: TimeString::try_from(row.created)?,
        })
    }

    async fn delete_from_database(token: Self::DatabaseKey) -> Result<(), Error> {
        sqlx::query!(r#"DELETE FROM "mcp_tokens" WHERE "token" = ?"#, token)
            .execute(db())
            .await?;

        Ok(())
    }
}
