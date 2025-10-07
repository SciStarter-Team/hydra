use anyhow::Error;
use common::shared::db;
use jiff::Timestamp;
use ulid::Ulid;

use crate::peering::LoadStorePeered;

/// Featured item with schedule details and entity information
#[derive(Debug, Clone, serde::Serialize)]
pub struct FeaturedItem {
    pub schedule_id: Ulid,
    pub title: String,
    pub content: Option<String>,
    pub url: Option<String>,
    pub entity_name: String,
    pub entity_image: String,
    pub entity_slug: String,
}

#[derive(
    Debug,
    Copy,
    Clone,
    sqlx::Type,
    serde::Serialize,
    serde::Deserialize,
    strum::AsRefStr,
    strum::EnumString,
    strum::Display,
)]
#[sqlx(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ScheduleType {
    Featured,
    Notification,
    Advertisement,
}

#[derive(
    Debug,
    Copy,
    Clone,
    sqlx::Type,
    serde::Serialize,
    serde::Deserialize,
    strum::AsRefStr,
    strum::EnumString,
    strum::Display,
)]
#[sqlx(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum TargetType {
    Slug,
    Global,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Schedule {
    pub schedule_id: Ulid,
    pub schedule_type: ScheduleType,
    pub target_type: Option<TargetType>,
    pub target: Option<String>,
    pub featured_entity: Option<String>,
    pub title: String,
    pub content: Option<String>,
    pub url: Option<String>,
    pub start_date: String,
    pub end_date: String,
    pub active: bool,
}

struct ScheduleDB {
    schedule_id: String,
    schedule_type: ScheduleType,
    target_type: Option<TargetType>,
    target: Option<String>,
    featured_entity: Option<String>,
    title: String,
    content: Option<String>,
    url: Option<String>,
    start_date: String,
    end_date: String,
    active: i64,
}

impl TryFrom<ScheduleDB> for Schedule {
    type Error = anyhow::Error;

    fn try_from(value: ScheduleDB) -> Result<Self, Self::Error> {
        Ok(Schedule {
            schedule_id: value.schedule_id.parse()?,
            schedule_type: value.schedule_type,
            target_type: value.target_type,
            target: value.target,
            featured_entity: value.featured_entity,
            title: value.title,
            content: value.content,
            url: value.url,
            start_date: value.start_date,
            end_date: value.end_date,
            active: value.active != 0,
        })
    }
}

impl Schedule {
    pub fn new(
        schedule_type: ScheduleType,
        target_type: Option<TargetType>,
        target: Option<String>,
        featured_entity: Option<String>,
        title: String,
        content: Option<String>,
        url: Option<String>,
        start_date: String,
        end_date: String,
    ) -> Self {
        Schedule {
            schedule_id: Ulid::new(),
            schedule_type,
            target_type,
            target,
            featured_entity,
            title,
            content,
            url,
            start_date,
            end_date,
            active: true,
        }
    }

    /// Get currently active featured items with entity details
    pub async fn get_current_featured_items() -> Result<Vec<FeaturedItem>, Error> {
        let now = Timestamp::now().strftime("%Y-%m-%d").to_string();
        let schedule_type = "featured";

        let items_raw = sqlx::query!(
            r#"SELECT s.schedule_id,
                      s.title,
                      s.content,
                      s.url,
                      s.featured_entity,
                      e.kind as entity_kind,
                      e.target as entity_target,
                      COALESCE(p.name, ev.name, '') as "entity_name: String",
                      COALESCE(p.image, ev.image, '') as "entity_image: String"
               FROM schedules s
               LEFT JOIN entities e ON s.featured_entity = e.slug
               LEFT JOIN projects p ON e.kind = 'Project' AND e.target = p.project_id
               LEFT JOIN events ev ON e.kind = 'Event' AND e.target = ev.event_id
               WHERE s.schedule_type = ?1
                 AND s.active = 1
                 AND s.start_date <= ?2
                 AND s.end_date >= ?2
                 AND s.featured_entity IS NOT NULL
               ORDER BY s.start_date DESC"#,
            schedule_type,
            now
        )
        .fetch_all(db())
        .await?;

        Ok(items_raw
            .into_iter()
            .filter_map(|row| {
                let schedule_id = row.schedule_id.parse().ok()?;
                let entity_slug = row.featured_entity?;
                let entity_name = row.entity_name;
                let entity_image = row.entity_image;

                // Skip if entity name is empty (entity not found)
                if entity_name.is_empty() {
                    return None;
                }

                Some(FeaturedItem {
                    schedule_id,
                    title: row.title,
                    content: row.content,
                    url: row.url,
                    entity_name,
                    entity_image,
                    entity_slug,
                })
            })
            .collect())
    }
}

impl LoadStorePeered for Schedule {
    type DatabaseKey = Ulid;

    const GROUP: &str = "schedule";

    #[tracing::instrument]
    async fn read_from_database(schedule_id: Self::DatabaseKey) -> Result<Self, Error> {
        let schedule_id_str = schedule_id.to_string();
        Ok(sqlx::query_as!(
            ScheduleDB,
            r#"select
                 schedule_id,
                 schedule_type as "schedule_type: ScheduleType",
                 target_type as "target_type: TargetType",
                 target,
                 featured_entity,
                 title,
                 content,
                 url,
                 start_date,
                 end_date,
                 active
               from schedules
               where schedule_id = ?"#,
            schedule_id_str,
        )
        .fetch_one(db())
        .await?
        .try_into()?)
    }

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        let schedule_id_str = self.schedule_id.to_string();
        let active_i64 = if self.active { 1i64 } else { 0i64 };

        sqlx::query!(
            r#"insert or replace into schedules (
                 schedule_id,
                 schedule_type,
                 target_type,
                 target,
                 featured_entity,
                 title,
                 content,
                 url,
                 start_date,
                 end_date,
                 active
               )
               values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            schedule_id_str,
            self.schedule_type,
            self.target_type,
            self.target,
            self.featured_entity,
            self.title,
            self.content,
            self.url,
            self.start_date,
            self.end_date,
            active_i64,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    #[tracing::instrument]
    async fn delete_from_database(schedule_id: Self::DatabaseKey) -> Result<(), Error> {
        let schedule_id_str = schedule_id.to_string();
        sqlx::query!(r#"DELETE FROM schedules WHERE schedule_id = ?"#, schedule_id_str)
            .execute(db())
            .await?;
        Ok(())
    }
}
