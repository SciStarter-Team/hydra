use anyhow::Error;
use common::timestring::TimeString;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::peering::LoadStorePeered;
use common::shared::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectReview {
    pub project_id: Ulid,
    pub user_id: Ulid,
    pub text: String,
    pub reviewed_at: TimeString,
}

impl LoadStorePeered for ProjectReview {
    type DatabaseKey = (Ulid, Ulid); // (project_id, user_id)
    const GROUP: &str = "review";
    const SPECIFIC: &str = "project";

    async fn save_to_database(&self) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        let user_id_str = self.user_id.to_string();
        let reviewed_at_str = self.reviewed_at.to_string();

        sqlx::query!(
            r#"
            INSERT INTO project_reviews (project_id, user_id, text, reviewed_at)
            VALUES (?, ?, ?, ?)
            ON CONFLICT (project_id, user_id) DO UPDATE SET
                text = excluded.text,
                reviewed_at = excluded.reviewed_at
            "#,
            project_id_str,
            user_id_str,
            self.text,
            reviewed_at_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, Error> {
        let (project_id, user_id) = key;
        let project_id_str = project_id.to_string();
        let user_id_str = user_id.to_string();

        let row = sqlx::query!(
            r#"
            SELECT project_id, user_id, text, reviewed_at
            FROM project_reviews
            WHERE project_id = ? AND user_id = ?
            "#,
            project_id_str,
            user_id_str
        )
        .fetch_one(db())
        .await?;

        Ok(ProjectReview {
            project_id: Ulid::from_string(&row.project_id)?,
            user_id: Ulid::from_string(&row.user_id)?,
            text: row.text,
            reviewed_at: TimeString::try_from(row.reviewed_at)?,
        })
    }
}

impl ProjectReview {
    pub fn new(project_id: Ulid, user_id: Ulid, text: String) -> Self {
        Self {
            project_id,
            user_id,
            text,
            reviewed_at: TimeString::now(),
        }
    }

    /// Get recent reviews for a project, ordered by most recent first
    ///
    /// # Arguments
    /// * `project_id` - The project to get reviews for
    /// * `limit` - Maximum number of reviews to return (default 10 if None)
    pub async fn list_for_project(project_id: Ulid, limit: Option<i64>) -> Result<Vec<Self>, Error> {
        let project_id_str = project_id.to_string();
        let limit = limit.unwrap_or(10);

        let rows = sqlx::query!(
            r#"
            SELECT project_id, user_id, text, reviewed_at
            FROM project_reviews
            WHERE project_id = ?
            ORDER BY reviewed_at DESC
            LIMIT ?
            "#,
            project_id_str,
            limit
        )
        .fetch_all(db())
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(ProjectReview {
                    project_id: Ulid::from_string(&row.project_id)?,
                    user_id: Ulid::from_string(&row.user_id)?,
                    text: row.text,
                    reviewed_at: TimeString::try_from(row.reviewed_at)?,
                })
            })
            .collect()
    }
}
