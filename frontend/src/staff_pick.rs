use anyhow::Error;
use common::shared::db;
use ulid::Ulid;

use crate::peering::LoadStorePeered;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StaffPick {
    pub id: Ulid,
    pub project_slug: String,
    pub label: String,
    pub description: String,
    pub location: String,
    pub acts_per_hour: String,
    pub picker_name: String,
    pub picker_image: String,
    pub picker_quote: String,
    pub priority: i32,
    pub begin_date: String,
    pub end_date: String,
}

struct StaffPickDB {
    id: String,
    project_slug: String,
    label: String,
    description: String,
    location: String,
    acts_per_hour: String,
    picker_name: String,
    picker_image: String,
    picker_quote: String,
    priority: i32,
    begin_date: String,
    end_date: String,
}

impl TryFrom<StaffPickDB> for StaffPick {
    type Error = anyhow::Error;

    fn try_from(row: StaffPickDB) -> Result<Self, Self::Error> {
        Ok(StaffPick {
            id: row.id.parse()?,
            project_slug: row.project_slug,
            label: row.label,
            description: row.description,
            location: row.location,
            acts_per_hour: row.acts_per_hour,
            picker_name: row.picker_name,
            picker_image: row.picker_image,
            picker_quote: row.picker_quote,
            priority: row.priority,
            begin_date: row.begin_date,
            end_date: row.end_date,
        })
    }
}

impl StaffPick {
    pub fn url(&self) -> String {
        format!("/{}", self.project_slug)
    }

    pub fn is_active(&self) -> bool {
        let today = jiff::Zoned::now().date().to_string();
        self.begin_date <= today && today <= self.end_date
    }

    pub async fn get_active_picks() -> Result<Vec<StaffPick>, Error> {
        let today = jiff::Zoned::now().date().to_string();
        let rows = sqlx::query_as!(
            StaffPickDB,
            r#"SELECT id, project_slug, label, description, location,
                      acts_per_hour, picker_name, picker_image, picker_quote,
                      priority as "priority: i32", begin_date, end_date
               FROM csm_staff_picks
               WHERE begin_date <= ?1 AND end_date >= ?1
               ORDER BY priority DESC"#,
            today,
        )
        .fetch_all(db())
        .await?;

        rows.into_iter()
            .map(StaffPick::try_from)
            .collect::<Result<Vec<_>, _>>()
    }
}

impl LoadStorePeered for StaffPick {
    type DatabaseKey = Ulid;

    const GROUP: &str = "staff_pick";

    #[tracing::instrument]
    async fn read_from_database(id: Self::DatabaseKey) -> Result<Self, Error> {
        let id_str = id.to_string();
        Ok(sqlx::query_as!(
            StaffPickDB,
            r#"SELECT id, project_slug, label, description, location,
                      acts_per_hour, picker_name, picker_image, picker_quote,
                      priority as "priority: i32", begin_date, end_date
               FROM csm_staff_picks
               WHERE id = ?"#,
            id_str,
        )
        .fetch_one(db())
        .await?
        .try_into()?)
    }

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        let id_str = self.id.to_string();

        sqlx::query!(
            r#"INSERT OR REPLACE INTO csm_staff_picks
               (id, project_slug, label, description, location, acts_per_hour,
                picker_name, picker_image, picker_quote, priority, begin_date, end_date)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            id_str,
            self.project_slug,
            self.label,
            self.description,
            self.location,
            self.acts_per_hour,
            self.picker_name,
            self.picker_image,
            self.picker_quote,
            self.priority,
            self.begin_date,
            self.end_date,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    #[tracing::instrument]
    async fn delete_from_database(id: Self::DatabaseKey) -> Result<(), Error> {
        let id_str = id.to_string();
        sqlx::query!("DELETE FROM csm_staff_picks WHERE id = ?", id_str)
            .execute(db())
            .await?;
        Ok(())
    }
}
