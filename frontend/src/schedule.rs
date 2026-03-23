use anyhow::Error;
use common::shared::db;
use jiff::Timestamp;
use ulid::Ulid;

use crate::{entity::EntityKind, peering::LoadStorePeered};

/// Featured item with all data needed for display in the homepage card.
/// Schedule fields (title, content, url) override entity fields when present.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FeaturedItem {
    pub schedule_id: Ulid,
    pub entity_slug: String,
    pub entity_kind: EntityKind,
    /// Display name (schedule title overrides entity name)
    pub name: String,
    pub image: String,
    pub presenter: String,
    /// Display description (schedule content overrides entity description)
    pub description: String,
    /// Custom URL from schedule, if any (otherwise link to entity page)
    pub custom_url: Option<String>,
    pub location_display: String,
    pub timing_display: String,
    pub topics: Vec<String>,
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
                      s.title as schedule_title,
                      s.content as schedule_content,
                      s.url as schedule_url,
                      s.featured_entity,
                      e.kind as "entity_kind: EntityKind",
                      e.target as entity_target,
                      COALESCE(p.name, ev.name, '') as "entity_name: String",
                      COALESCE(p.image, ev.image, '') as "entity_image: String",
                      COALESCE(p.presenter, ev.presenter, '') as "presenter: String",
                      COALESCE(p.description, ev.description, '') as "entity_description: String",
                      COALESCE(p.location_type, ev.location_type, '') as "location_type: String",
                      COALESCE(p.entirely_online, ev.entirely_online, 0) as "entirely_online: bool",
                      COALESCE(p.city, ev.city, '') as "city: String",
                      COALESCE(p.state, ev.state, '') as "state: String",
                      COALESCE(p.country, ev.country, '') as "country: String",
                      p.project_timing as "project_timing: String",
                      (SELECT GROUP_CONCAT(topic, ', ')
                       FROM project_topics
                       WHERE project_id = e.target
                         AND e.kind = 'Project') as "topics: String",
                      (SELECT ei.begin
                       FROM event_instances ei
                       WHERE ei.event_id = e.target
                         AND e.kind = 'Event'
                         AND ei.begin >= ?2
                       ORDER BY ei.begin ASC
                       LIMIT 1) as "next_instance_begin: String",
                      (SELECT ei.end
                       FROM event_instances ei
                       WHERE ei.event_id = e.target
                         AND e.kind = 'Event'
                         AND ei.begin >= ?2
                       ORDER BY ei.begin ASC
                       LIMIT 1) as "next_instance_end: String",
                      (SELECT ei.begin
                       FROM event_instances ei
                       WHERE ei.event_id = e.target
                         AND e.kind = 'Event'
                         AND ei.end < ?2
                       ORDER BY ei.end DESC
                       LIMIT 1) as "last_instance_begin: String"
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
                let entity_kind = row.entity_kind?;

                // Schedule title overrides entity name when non-empty
                let name = if row.schedule_title.is_empty() {
                    row.entity_name.clone()
                } else {
                    row.schedule_title
                };

                // Skip if we have no name at all
                if name.is_empty() {
                    return None;
                }

                // Schedule content overrides entity description when non-empty
                let description = row
                    .schedule_content
                    .filter(|s| !s.is_empty())
                    .unwrap_or(row.entity_description);

                // Schedule URL provides custom link (None means link to entity page)
                let custom_url = row.schedule_url.filter(|u| !u.is_empty());

                // Compute location display string
                let location_display = compute_location_display(
                    &row.location_type,
                    row.entirely_online,
                    &row.city,
                    &row.state,
                    &row.country,
                );

                // Compute timing display string
                let timing_display = compute_timing_display(
                    &entity_kind,
                    row.project_timing.as_deref(),
                    row.next_instance_begin.as_deref(),
                    row.next_instance_end.as_deref(),
                    row.last_instance_begin.as_deref(),
                );

                // Parse topics from comma-separated string
                let topics = row
                    .topics
                    .map(|t| t.split(", ").map(String::from).collect())
                    .unwrap_or_default();

                Some(FeaturedItem {
                    schedule_id,
                    entity_slug,
                    entity_kind,
                    name,
                    image: row.entity_image,
                    presenter: row.presenter,
                    description,
                    custom_url,
                    location_display,
                    timing_display,
                    topics,
                })
            })
            .collect())
    }
}

/// Compute human-readable location display from location fields
fn compute_location_display(
    location_type: &str,
    entirely_online: bool,
    city: &str,
    state: &str,
    country: &str,
) -> String {
    // If entirely online or global location type
    if entirely_online || location_type == "Global, anywhere on the planet" {
        return "Anywhere".to_string();
    }

    // If we have city/state info
    if !city.is_empty() && !state.is_empty() {
        return format!("{}, {}", city, state);
    }

    if !city.is_empty() {
        return city.to_string();
    }

    if !state.is_empty() {
        return state.to_string();
    }

    if !country.is_empty() {
        return country.to_string();
    }

    // Default
    "Location varies".to_string()
}

/// Compute human-readable timing display
fn compute_timing_display(
    entity_kind: &EntityKind,
    project_timing: Option<&str>,
    next_instance_begin: Option<&str>,
    next_instance_end: Option<&str>,
    last_instance_begin: Option<&str>,
) -> String {
    match entity_kind {
        EntityKind::Project => {
            // Use project_timing if available
            if let Some(timing) = project_timing {
                if !timing.is_empty() {
                    return timing.to_string();
                }
            }
            "Ongoing".to_string()
        }
        EntityKind::Event => {
            // Try to show the next upcoming instance
            if let Some(begin) = next_instance_begin {
                return format_event_date(begin, next_instance_end);
            }
            // If no upcoming instance, show the last one as past
            if let Some(begin) = last_instance_begin {
                return format!("Was {}", format_event_date(begin, None));
            }
            // No instances at all
            "Date TBD".to_string()
        }
        _ => String::new(),
    }
}

/// Format an event date for display
fn format_event_date(begin: &str, end: Option<&str>) -> String {
    // Parse the date string (expected format: YYYY-MM-DD or ISO timestamp)
    let begin_date = begin.split('T').next().unwrap_or(begin);

    if let Ok(date) = jiff::civil::Date::strptime("%Y-%m-%d", begin_date) {
        let formatted = date.strftime("%b %d, %Y").to_string();

        // If end date is different, show range
        if let Some(end) = end {
            let end_date = end.split('T').next().unwrap_or(end);
            if end_date != begin_date {
                if let Ok(end_parsed) = jiff::civil::Date::strptime("%Y-%m-%d", end_date) {
                    return format!("{} - {}", formatted, end_parsed.strftime("%b %d, %Y"));
                }
            }
        }

        formatted
    } else {
        // Fallback: return as-is
        begin.to_string()
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
        sqlx::query!(
            r#"DELETE FROM schedules WHERE schedule_id = ?"#,
            schedule_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }
}
