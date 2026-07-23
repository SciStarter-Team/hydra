use anyhow::Error;
use common::{shared::db, timestring::TimeString, transaction::Immediate};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::peering::LoadStorePeered;

/// How long a conversation is retained after its last turn.
const RETENTION_HOURS: i64 = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptRole {
    User,
    Assistant,
}

/// One content block within a transcript message. Modeled independently of
/// the AWS SDK types so persisted conversations don't depend on SDK details.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptContent {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        id: String,
        json: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptMsg {
    pub role: TranscriptRole,
    pub content: Vec<TranscriptContent>,
}

/// A concierge chat conversation, synchronized to all frontends so that a
/// turn may be handled by any instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub conversation_id: Ulid,
    pub created: String,
    pub updated: String,
    pub user_id: Option<Ulid>,
    pub turns: u32,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub transcript: Vec<TranscriptMsg>,
}

impl Conversation {
    pub fn new(user_id: Option<Ulid>) -> Self {
        let now = TimeString::now().to_string();
        Conversation {
            conversation_id: Ulid::new(),
            created: now.clone(),
            updated: now,
            user_id,
            turns: 0,
            tokens_in: 0,
            tokens_out: 0,
            transcript: Vec::new(),
        }
    }
}

impl LoadStorePeered for Conversation {
    type DatabaseKey = Ulid;

    const GROUP: &str = "concierge";
    const SPECIFIC: &str = "conversation";

    async fn save_to_database(&self) -> Result<(), Error> {
        let conversation_id = self.conversation_id.to_string();
        let user_id = self.user_id.map(|id| id.to_string());
        let turns = self.turns as i64;
        let transcript = serde_json::to_string(&self.transcript)?;

        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;
        sqlx::query!(
            r#"INSERT OR REPLACE INTO "concierge_conversations"
               ("conversation_id", "created", "updated", "user_id",
                "turns", "tokens_in", "tokens_out", "transcript")
               VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
            conversation_id,
            self.created,
            self.updated,
            user_id,
            turns,
            self.tokens_in,
            self.tokens_out,
            transcript,
        )
        .execute(&mut tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn read_from_database(key: Ulid) -> Result<Self, Error> {
        let conversation_id = key.to_string();
        let row = sqlx::query!(
            r#"SELECT "created", "updated", "user_id", "turns",
                      "tokens_in", "tokens_out", "transcript"
               FROM "concierge_conversations" WHERE "conversation_id" = ?"#,
            conversation_id,
        )
        .fetch_one(db())
        .await?;

        Ok(Conversation {
            conversation_id: key,
            created: row.created,
            updated: row.updated,
            user_id: row.user_id.and_then(|id| id.parse().ok()),
            turns: row.turns as u32,
            tokens_in: row.tokens_in,
            tokens_out: row.tokens_out,
            transcript: serde_json::from_str(&row.transcript)?,
        })
    }

    async fn delete_from_database(key: Ulid) -> Result<(), Error> {
        let conversation_id = key.to_string();
        sqlx::query!(
            r#"DELETE FROM "concierge_conversations" WHERE "conversation_id" = ?"#,
            conversation_id,
        )
        .execute(db())
        .await?;
        Ok(())
    }
}

/// Periodically delete expired conversations from the local database.
///
/// Every instance holds its own peered copy and runs this loop, so no
/// leader election or peer notification is needed: each instance removes
/// the same expired rows from its own database.
pub async fn expire_conversations() {
    loop {
        let cutoff = (jiff::Timestamp::now() - jiff::Span::new().hours(RETENTION_HOURS))
            .to_string();
        match sqlx::query!(
            r#"DELETE FROM "concierge_conversations" WHERE "updated" < ?"#,
            cutoff,
        )
        .execute(db())
        .await
        {
            Ok(result) => {
                if result.rows_affected() > 0 {
                    tracing::info!(
                        "Expired {} concierge conversations",
                        result.rows_affected()
                    );
                }
            }
            Err(e) => tracing::warn!("Concierge conversation expiry failed: {}", e),
        }

        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}
