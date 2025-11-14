use anyhow::Error;
use common::timestring::TimeString;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::peering::LoadStorePeered;
use common::shared::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRating {
    pub project_id: Ulid,
    pub user_id: Ulid,
    pub rated_at: TimeString,
}

impl LoadStorePeered for ProjectRating {
    type DatabaseKey = (Ulid, Ulid); // (project_id, user_id)
    const GROUP: &str = "rating";
    const SPECIFIC: &str = "project";

    async fn save_to_database(&self) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        let user_id_str = self.user_id.to_string();
        let rated_at_str = self.rated_at.to_string();

        sqlx::query!(
            r#"
            INSERT INTO project_ratings (project_id, user_id, rated_at)
            VALUES (?, ?, ?)
            ON CONFLICT (project_id, user_id) DO NOTHING
            "#,
            project_id_str,
            user_id_str,
            rated_at_str
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
            SELECT project_id, user_id, rated_at
            FROM project_ratings
            WHERE project_id = ? AND user_id = ?
            "#,
            project_id_str,
            user_id_str
        )
        .fetch_one(db())
        .await?;

        Ok(ProjectRating {
            project_id: Ulid::from_string(&row.project_id)?,
            user_id: Ulid::from_string(&row.user_id)?,
            rated_at: TimeString::try_from(row.rated_at)?,
        })
    }
}

impl ProjectRating {
    pub fn new(project_id: Ulid, user_id: Ulid) -> Self {
        Self {
            project_id,
            user_id,
            rated_at: TimeString::now(),
        }
    }

    /// Get count of ratings for a project
    pub async fn count_for_project(project_id: Ulid) -> Result<i64, Error> {
        let project_id_str = project_id.to_string();

        let row = sqlx::query!(
            "SELECT COUNT(*) as count FROM project_ratings WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;

        Ok(row.count)
    }

    /// Check if user has rated a project
    pub async fn has_rated(project_id: Ulid, user_id: Ulid) -> Result<bool, Error> {
        let project_id_str = project_id.to_string();
        let user_id_str = user_id.to_string();

        let row = sqlx::query!(
            "SELECT COUNT(*) as count FROM project_ratings WHERE project_id = ? AND user_id = ?",
            project_id_str,
            user_id_str
        )
        .fetch_one(db())
        .await?;

        Ok(row.count > 0)
    }
}
