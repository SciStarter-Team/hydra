use anyhow::Error;
use common::{shared::db, timestring::TimeString};
use jiff::{Timestamp, tz::TimeZone};
use ulid::Ulid;

use crate::{
    component::html_escape,
    entity::EntityKind,
    local_time::{Clocks, TimeFormat, zoned_time, zoned_time_range},
    peering::LoadStorePeered,
};

/// Featured item with all data needed for display in the homepage card.
/// Schedule fields (title, content, url) override entity fields when present.
#[derive(Debug, Clone)]
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
    pub timing: Timing,
    pub topics: Vec<String>,
}

impl FeaturedItem {
    /// The link target for the card: the schedule's custom URL if set,
    /// otherwise the entity's canonical path (`/projects/<slug>`,
    /// `/events/<slug>`, or `/<slug>`).
    pub fn link(&self) -> String {
        match &self.custom_url {
            Some(url) => url.clone(),
            None => format!("{}{}", self.entity_kind.path_prefix(), self.entity_slug),
        }
    }
}

/// When a featured item happens, as its card tells it.
#[derive(Debug, Clone, PartialEq)]
pub enum Timing {
    /// A project's timing, in its own words.
    Described(String),
    /// An event's session that is under way or next to come: its start, and
    /// its end, which is its start when it was given none. Told on the
    /// clocks of its venue's `zone`, or the viewer's when it has none.
    Next {
        begin: String,
        end: String,
        zone: Option<TimeZone>,
    },
    /// An event whose sessions are all over: when the last of them began.
    Past {
        begin: String,
        zone: Option<TimeZone>,
    },
    /// An event with no sessions.
    Unscheduled,
}

impl Timing {
    /// The card's line, as HTML.
    pub fn html(&self) -> String {
        match self {
            Timing::Described(text) => html_escape(text),
            Timing::Next { begin, end, zone } => {
                zoned_time_range(begin, end, TimeFormat::ShortDate, Clocks::of(zone.as_ref()))
            }
            Timing::Past { begin, zone } => format!(
                "Was {}",
                zoned_time(begin, TimeFormat::ShortDate, Clocks::of(zone.as_ref()))
            ),
            Timing::Unscheduled => "Date TBD".to_string(),
        }
    }
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

/// A compact featured-schedule row for admin listings: the id needed to edit
/// it, plus title/active/date-range. `get_current_featured_items` returns
/// display cards without ids; this exposes the editable rows.
#[derive(Debug, Clone)]
pub struct FeaturedScheduleRow {
    pub schedule_id: Ulid,
    pub title: String,
    pub active: bool,
    pub start_date: String,
    pub end_date: String,
}

impl Schedule {
    /// All `featured` schedules as editable rows, newest first.
    pub async fn list_featured_admin() -> Result<Vec<FeaturedScheduleRow>, Error> {
        let rows = sqlx::query!(
            r#"SELECT schedule_id, title, active, start_date, end_date
               FROM schedules
               WHERE schedule_type = 'featured'
               ORDER BY start_date DESC"#
        )
        .fetch_all(db())
        .await?;

        rows.into_iter()
            .map(|r| {
                Ok(FeaturedScheduleRow {
                    schedule_id: r.schedule_id.parse()?,
                    title: r.title,
                    active: r.active != 0,
                    start_date: r.start_date,
                    end_date: r.end_date,
                })
            })
            .collect()
    }

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
        let today = Timestamp::now().strftime("%Y-%m-%d").to_string();
        let now = TimeString::now().to_string();
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
                      (SELECT GROUP_CONCAT(cn.name, ', ' ORDER BY cn.position, cn.name)
                       FROM choice_names cn
                       WHERE cn.target = e.target
                         AND cn.list = 'Topic'
                         AND e.kind = 'Project') as "topics: String",
                      (SELECT ei.begin
                       FROM event_instances ei
                       WHERE ei.event_id = e.target
                         AND e.kind = 'Event'
                         AND ei.end >= ?3
                       ORDER BY ei.begin ASC, ei.end ASC
                       LIMIT 1) as "next_instance_begin: String",
                      (SELECT ei.end
                       FROM event_instances ei
                       WHERE ei.event_id = e.target
                         AND e.kind = 'Event'
                         AND ei.end >= ?3
                       ORDER BY ei.begin ASC, ei.end ASC
                       LIMIT 1) as "next_instance_end: String",
                      (SELECT ei.begin
                       FROM event_instances ei
                       WHERE ei.event_id = e.target
                         AND e.kind = 'Event'
                         AND ei.end < ?3
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
            today,
            now
        )
        .fetch_all(db())
        .await?;

        // An event's times are on its venue's clocks, as on its page.
        let zones = crate::event::zone::of_events(
            items_raw
                .iter()
                .filter(|row| matches!(row.entity_kind, Some(EntityKind::Event)))
                .filter_map(|row| row.entity_target.as_deref()),
        )
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

                // Schedule content overrides entity description when non-empty.
                // Descriptions may be stored as Quill Delta JSON; render them as
                // plain text so the card never shows raw JSON (full length, as
                // featured blurbs were never truncated).
                let description = row
                    .schedule_content
                    .filter(|s| !s.is_empty())
                    .unwrap_or(row.entity_description);
                let description = crate::search::description_excerpt(&description, usize::MAX);

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

                let zone = row
                    .entity_target
                    .as_deref()
                    .and_then(|target| zones.get(target))
                    .cloned();
                let timing = timing(
                    &entity_kind,
                    row.project_timing,
                    row.next_instance_begin.zip(row.next_instance_end),
                    row.last_instance_begin,
                    zone,
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
                    timing,
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
    if entirely_online || location_type == crate::project::location::ANYWHERE {
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

/// When a featured item happens: a project's timing as it describes it, an
/// event's session under way or next to come (`next`, its begin and end), or
/// else when the last of them began.
fn timing(
    entity_kind: &EntityKind,
    project_timing: Option<String>,
    next: Option<(String, String)>,
    last_begin: Option<String>,
    zone: Option<TimeZone>,
) -> Timing {
    match entity_kind {
        EntityKind::Project => Timing::Described(
            project_timing
                .filter(|timing| !timing.is_empty())
                .unwrap_or_else(|| "Ongoing".to_string()),
        ),
        EntityKind::Event => match (next, last_begin) {
            (Some((begin, end)), _) => Timing::Next { begin, end, zone },
            (None, Some(begin)) => Timing::Past { begin, zone },
            (None, None) => Timing::Unscheduled,
        },
        _ => Timing::Described(String::new()),
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

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    #[test]
    fn an_events_date_is_told_on_its_venues_clocks() {
        // 01:00 UTC on the 6th is still the evening of the 5th in Los Angeles.
        let zone = TimeZone::get("America/Los_Angeles").ok();
        let next = timing(
            &EntityKind::Event,
            None,
            Some((
                "2027-03-06T01:00:00Z".to_string(),
                "2027-03-06T03:00:00Z".to_string(),
            )),
            None,
            zone,
        );
        check!(next.html().contains(">Mar 5, 2027</time>"));
    }

    #[test]
    fn a_session_with_no_end_is_one_date() {
        let next = timing(
            &EntityKind::Event,
            None,
            Some((
                "2027-03-06T15:00:00Z".to_string(),
                "2027-03-06T15:00:00Z".to_string(),
            )),
            None,
            None,
        );
        check!(!next.html().contains("data-end"));
    }

    #[test]
    fn an_event_whose_sessions_are_over_says_when_it_was() {
        let past = timing(
            &EntityKind::Event,
            None,
            None,
            Some("2026-03-06T15:00:00Z".to_string()),
            None,
        );
        check!(past.html().starts_with("Was <time"));
        check!(timing(&EntityKind::Event, None, None, None, None) == Timing::Unscheduled);
    }

    #[test]
    fn a_projects_timing_is_its_own_words_escaped() {
        let described = timing(
            &EntityKind::Project,
            Some("Spring & <fall>".to_string()),
            None,
            None,
            None,
        );
        check!(described.html() == "Spring &amp; &lt;fall&gt;");
        check!(
            timing(&EntityKind::Project, Some(String::new()), None, None, None).html() == "Ongoing"
        );
    }
}
