use anyhow::Error;
use common::{message::user::BasicInfo, service::log_activity, shared::db, timestring::TimeString};
use poem::{
    IntoResponse, handler,
    web::{Html, Path},
};
use sailfish::TemplateOnce;
use ulid::Ulid;

use std::collections::HashMap;
use std::sync::LazyLock;

use common::message::user::{UserByEmailRequest, UserByEmailResponse};
use common::service::request;

use crate::admin::autocomplete::resolve_user_names;
use crate::component::facebook_questions::{self, FacebookAnswers};
use crate::component::floating_field::FormError;
use crate::contact::{Contact, ContactRole};
use crate::event::EventType;
use crate::event::facebook::FacebookEvent;
use crate::event::field::{EventField, EventFieldType, EventFieldValue};
use crate::local_time::{TimeFormat, local_time};
use crate::middleware::authorization::Authenticated;
use crate::page_change::record_edit;
use crate::participant_file::ParticipantFile;
use crate::peering::LoadStorePeered;
use crate::project::field::{
    AUDIENCE_VOCABULARY, DIFFICULTY_VOCABULARY, ProjectField, ProjectFieldType, ProjectFieldValue,
};
use crate::project::field_source::FieldSource;

use super::detail_page::DetailPageKind;

// ---------------------------------------------------------------------------
// Field metadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum FieldInputKind {
    Text,
    Textarea,
    RichText,
    Url,
    Image,
    Boolean,
    Select(&'static [(&'static str, &'static str)]),
    MultiCheckbox(&'static [(&'static str, &'static str)]),
}

#[derive(Debug, Clone)]
pub struct FieldMeta {
    pub label: &'static str,
    pub input_kind: FieldInputKind,
}

/// Every event type, as (stored value, human label).
///
/// Built from [`EventType::ALL`] rather than written out, because a hand-kept
/// copy drifted: it once offered "Festival", "Meeting" and "Workshop", none of
/// which are variants, so choosing one wrote a string into `events.event_type`
/// that no reader could decode — while most of the real types were unreachable.
static EVENT_TYPE_OPTIONS: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    EventType::ALL
        .iter()
        .map(|event_type| (event_type.storage_name(), event_type.label()))
        .collect()
});

/// The audiences a project may name, built from the vocabulary every other
/// reader and writer of `project_audiences` uses, for the reason given at
/// [`EVENT_TYPE_OPTIONS`].
static AUDIENCE_OPTIONS: LazyLock<Vec<(&'static str, &'static str)>> =
    LazyLock::new(|| vocabulary_options(AUDIENCE_VOCABULARY));

/// The difficulties a project may have, from the same vocabulary as the
/// admin's.
static DIFFICULTY_OPTIONS: LazyLock<Vec<(&'static str, &'static str)>> =
    LazyLock::new(|| vocabulary_options(DIFFICULTY_VOCABULARY));

/// Options whose values are their own labels.
fn vocabulary_options(vocabulary: &[&'static str]) -> Vec<(&'static str, &'static str)> {
    vocabulary.iter().map(|value| (*value, *value)).collect()
}

/// Map field_type string to its metadata. Returns None for unknown fields.
pub fn field_meta(field_type: &str, kind: DetailPageKind) -> Option<FieldMeta> {
    match (field_type, kind) {
        // Common fields (both projects and events)
        ("name", _) => Some(FieldMeta {
            label: "Name",
            input_kind: FieldInputKind::Text,
        }),
        ("presenter", _) => Some(FieldMeta {
            label: "Presenter",
            input_kind: FieldInputKind::Text,
        }),
        ("image", _) => Some(FieldMeta {
            label: "Image",
            input_kind: FieldInputKind::Image,
        }),
        ("image_credit", _) => Some(FieldMeta {
            label: "Image Credit",
            input_kind: FieldInputKind::Text,
        }),
        ("url", _) => Some(FieldMeta {
            label: "URL",
            input_kind: FieldInputKind::Url,
        }),
        ("description", _) => Some(FieldMeta {
            label: "Description",
            input_kind: FieldInputKind::RichText,
        }),
        ("how_to_join", _) => Some(FieldMeta {
            label: "How to Get Started",
            input_kind: FieldInputKind::RichText,
        }),
        ("location_name", _) => Some(FieldMeta {
            label: "Location Name",
            input_kind: FieldInputKind::Text,
        }),
        ("location_description", _) => Some(FieldMeta {
            label: "Location Description",
            input_kind: FieldInputKind::Textarea,
        }),
        ("address", _) => Some(FieldMeta {
            label: "Address",
            input_kind: FieldInputKind::Text,
        }),
        ("city", _) => Some(FieldMeta {
            label: "City",
            input_kind: FieldInputKind::Text,
        }),
        ("state", _) => Some(FieldMeta {
            label: "State",
            input_kind: FieldInputKind::Text,
        }),
        ("zip", _) => Some(FieldMeta {
            label: "Zip",
            input_kind: FieldInputKind::Text,
        }),
        ("country", _) => Some(FieldMeta {
            label: "Country",
            input_kind: FieldInputKind::Text,
        }),
        ("entirely_online", _) => Some(FieldMeta {
            label: "Entirely Online",
            input_kind: FieldInputKind::Boolean,
        }),

        // Project-only displayed fields
        ("goal", DetailPageKind::Project) => Some(FieldMeta {
            label: "Goal",
            input_kind: FieldInputKind::Textarea,
        }),
        ("task", DetailPageKind::Project) => Some(FieldMeta {
            label: "Task",
            input_kind: FieldInputKind::Textarea,
        }),
        ("video_url", DetailPageKind::Project) => Some(FieldMeta {
            label: "Video URL",
            input_kind: FieldInputKind::Url,
        }),
        ("total_expense", DetailPageKind::Project) => Some(FieldMeta {
            label: "Total Expense",
            input_kind: FieldInputKind::Text,
        }),
        ("time_per_participation", DetailPageKind::Project) => Some(FieldMeta {
            label: "Time Per Participation",
            input_kind: FieldInputKind::Text,
        }),
        ("participation_schedule", DetailPageKind::Project) => Some(FieldMeta {
            label: "Participation Schedule",
            input_kind: FieldInputKind::Text,
        }),
        ("mobile_app_url", DetailPageKind::Project) => Some(FieldMeta {
            label: "Mobile App URL",
            input_kind: FieldInputKind::Url,
        }),
        ("mobile_app_url_ios", DetailPageKind::Project) => Some(FieldMeta {
            label: "iOS App URL",
            input_kind: FieldInputKind::Url,
        }),
        ("mobile_app_url_android", DetailPageKind::Project) => Some(FieldMeta {
            label: "Android App URL",
            input_kind: FieldInputKind::Url,
        }),
        ("training_materials", DetailPageKind::Project) => Some(FieldMeta {
            label: "Training Materials URL",
            input_kind: FieldInputKind::Url,
        }),
        ("classroom_materials", DetailPageKind::Project) => Some(FieldMeta {
            label: "Classroom Materials URL",
            input_kind: FieldInputKind::Url,
        }),
        ("twitter_name", DetailPageKind::Project) => Some(FieldMeta {
            label: "Twitter/X Handle",
            input_kind: FieldInputKind::Text,
        }),
        ("facebook_page", DetailPageKind::Project) => Some(FieldMeta {
            label: "Facebook Page",
            input_kind: FieldInputKind::Text,
        }),
        ("instagram_page", DetailPageKind::Project) => Some(FieldMeta {
            label: "Instagram Page",
            input_kind: FieldInputKind::Text,
        }),
        ("tags", DetailPageKind::Project) => Some(FieldMeta {
            label: "Tags",
            input_kind: FieldInputKind::Text,
        }),
        ("audiences", DetailPageKind::Project) => Some(FieldMeta {
            label: "Audiences",
            input_kind: FieldInputKind::MultiCheckbox(&AUDIENCE_OPTIONS),
        }),

        // Project additional fields
        ("status", DetailPageKind::Project) => Some(FieldMeta {
            label: "Status",
            input_kind: FieldInputKind::Select(&[
                ("Active", "Active"),
                ("PeriodicallyActive", "Periodically Active"),
                ("Complete", "Complete"),
                ("Hiatus", "Hiatus"),
                ("Pending", "Pending"),
                ("Hidden", "Hidden"),
            ]),
        }),
        ("difficulty", DetailPageKind::Project) => Some(FieldMeta {
            label: "Difficulty",
            input_kind: FieldInputKind::Select(&DIFFICULTY_OPTIONS),
        }),
        ("search_terms", DetailPageKind::Project) => Some(FieldMeta {
            label: "Search Terms",
            input_kind: FieldInputKind::Textarea,
        }),
        ("signup_url", DetailPageKind::Project) => Some(FieldMeta {
            label: "Signup URL",
            input_kind: FieldInputKind::Url,
        }),
        ("opengraph_description", DetailPageKind::Project) => Some(FieldMeta {
            label: "OpenGraph Description",
            input_kind: FieldInputKind::Textarea,
        }),
        ("begin", DetailPageKind::Project) => Some(FieldMeta {
            label: "Begin Date",
            input_kind: FieldInputKind::Text,
        }),
        ("end", DetailPageKind::Project) => Some(FieldMeta {
            label: "End Date",
            input_kind: FieldInputKind::Text,
        }),
        ("project_timing", DetailPageKind::Project) => Some(FieldMeta {
            label: "Project Timing",
            input_kind: FieldInputKind::Text,
        }),
        ("year_started", DetailPageKind::Project) => Some(FieldMeta {
            label: "Year Started",
            input_kind: FieldInputKind::Text,
        }),
        ("on_demand", DetailPageKind::Project) => Some(FieldMeta {
            label: "On Demand",
            input_kind: FieldInputKind::Boolean,
        }),
        ("in_person", DetailPageKind::Project) => Some(FieldMeta {
            label: "In Person",
            input_kind: FieldInputKind::Boolean,
        }),
        ("indoors", DetailPageKind::Project) => Some(FieldMeta {
            label: "Indoors",
            input_kind: FieldInputKind::Boolean,
        }),
        ("outdoors", DetailPageKind::Project) => Some(FieldMeta {
            label: "Outdoors",
            input_kind: FieldInputKind::Boolean,
        }),
        ("regions_description", DetailPageKind::Project) => Some(FieldMeta {
            label: "Name of the geographic area",
            input_kind: FieldInputKind::Textarea,
        }),
        ("materials", DetailPageKind::Project) => Some(FieldMeta {
            label: "Materials",
            input_kind: FieldInputKind::Textarea,
        }),
        ("gear", DetailPageKind::Project) => Some(FieldMeta {
            label: "Gear",
            input_kind: FieldInputKind::Textarea,
        }),
        ("outcomes_label", DetailPageKind::Project) => Some(FieldMeta {
            label: "Outcomes Label",
            input_kind: FieldInputKind::Text,
        }),
        ("outcomes_link", DetailPageKind::Project) => Some(FieldMeta {
            label: "Outcomes Link",
            input_kind: FieldInputKind::Url,
        }),
        ("time_required", DetailPageKind::Project) => Some(FieldMeta {
            label: "Time Required",
            input_kind: FieldInputKind::Text,
        }),
        ("consent_statement", DetailPageKind::Project) => Some(FieldMeta {
            label: "Consent Statement",
            input_kind: FieldInputKind::Textarea,
        }),
        ("data_publication_url", DetailPageKind::Project) => Some(FieldMeta {
            label: "Data Publication URL",
            input_kind: FieldInputKind::Url,
        }),
        ("data_license", DetailPageKind::Project) => Some(FieldMeta {
            label: "Data License",
            input_kind: FieldInputKind::Text,
        }),
        ("university_affiliation", DetailPageKind::Project) => Some(FieldMeta {
            label: "University Affiliation",
            input_kind: FieldInputKind::Text,
        }),
        ("participation_degree", DetailPageKind::Project) => Some(FieldMeta {
            label: "Participation Degree",
            input_kind: FieldInputKind::Select(&[
                ("CoCreated", "Co-Created"),
                ("Collaborative", "Collaborative"),
                ("Contributory", "Contributory"),
                ("Contractual", "Contractual"),
            ]),
        }),

        // Event-only fields
        ("event_type", DetailPageKind::Event) => Some(FieldMeta {
            label: "Event Type",
            input_kind: FieldInputKind::Select(&EVENT_TYPE_OPTIONS),
        }),

        // Event additional fields
        ("status", DetailPageKind::Event) => Some(FieldMeta {
            label: "Status",
            input_kind: FieldInputKind::Select(&[("Active", "Active"), ("Cancelled", "Cancelled")]),
        }),

        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Permission helper
// ---------------------------------------------------------------------------

/// Check whether the user can edit the given project/event, by the rule
/// every edit follows ([`crate::edit_access`]).
/// Returns (can_edit, is_staff_editor).
pub async fn check_edit_permission(
    target_id: &str,
    user: &BasicInfo,
    kind: DetailPageKind,
) -> Result<(bool, bool), Error> {
    use crate::edit_access::{Editor, may_edit};

    let editor = Editor::from(user);

    let Ok(target) = Ulid::from_string(target_id) else {
        return Ok((false, editor.is_staff));
    };

    Ok((may_edit(kind.page(target), editor).await?, editor.is_staff))
}

/// Determine the kind (Project vs Event) for a target_id by checking which table it exists in.
async fn resolve_kind(target_id: &str) -> Result<DetailPageKind, Error> {
    let is_project = sqlx::query_scalar!(
        r#"SELECT 1 as "v!: i32" FROM projects WHERE project_id = ?"#,
        target_id
    )
    .fetch_optional(db())
    .await?
    .is_some();

    if is_project {
        Ok(DetailPageKind::Project)
    } else {
        Ok(DetailPageKind::Event)
    }
}

// ---------------------------------------------------------------------------
// Read current field value from database
// ---------------------------------------------------------------------------

async fn read_field_value(
    target_id: &str,
    field_type: &str,
    kind: DetailPageKind,
) -> Result<String, Error> {
    match kind {
        DetailPageKind::Project => read_project_field(target_id, field_type).await,
        DetailPageKind::Event => read_event_field(target_id, field_type).await,
    }
}

async fn read_project_field(id: &str, field_type: &str) -> Result<String, Error> {
    match field_type {
        "name" => Ok(
            sqlx::query_scalar!("SELECT name FROM projects WHERE project_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "presenter" => Ok(sqlx::query_scalar!(
            "SELECT presenter FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "image" => Ok(
            sqlx::query_scalar!("SELECT image FROM projects WHERE project_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "image_credit" => Ok(sqlx::query_scalar!(
            "SELECT image_credit FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "url" => Ok(
            sqlx::query_scalar!("SELECT url FROM projects WHERE project_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "goal" => Ok(
            sqlx::query_scalar!("SELECT goal FROM projects WHERE project_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "task" => Ok(
            sqlx::query_scalar!("SELECT task FROM projects WHERE project_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "description" => Ok(sqlx::query_scalar!(
            "SELECT description FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "how_to_join" => Ok(sqlx::query_scalar!(
            "SELECT how_to_join FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "location_name" => Ok(sqlx::query_scalar!(
            "SELECT location_name FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "location_description" => Ok(sqlx::query_scalar!(
            "SELECT location_description FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "address" => Ok(sqlx::query_scalar!(
            "SELECT address FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "city" => Ok(
            sqlx::query_scalar!("SELECT city FROM projects WHERE project_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "state" => Ok(
            sqlx::query_scalar!("SELECT state FROM projects WHERE project_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "zip" => Ok(
            sqlx::query_scalar!("SELECT zip FROM projects WHERE project_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "country" => Ok(sqlx::query_scalar!(
            "SELECT country FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "entirely_online" => {
            let v = sqlx::query_scalar!(
                r#"SELECT entirely_online as "v: bool" FROM projects WHERE project_id = ?"#,
                id
            )
            .fetch_one(db())
            .await?;
            Ok(if v { "true" } else { "false" }.to_string())
        }
        "video_url" => Ok(sqlx::query_scalar!(
            "SELECT video_url FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "total_expense" => {
            let v = sqlx::query_scalar!(
                "SELECT total_expense FROM projects WHERE project_id = ?",
                id
            )
            .fetch_one(db())
            .await?;
            Ok(v.map(|f| f.to_string()).unwrap_or_default())
        }
        "time_per_participation" => Ok(sqlx::query_scalar!(
            "SELECT time_per_participation FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "participation_schedule" => Ok(sqlx::query_scalar!(
            "SELECT participation_schedule FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "mobile_app_url" => Ok(sqlx::query_scalar!(
            "SELECT mobile_app_url FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "mobile_app_url_ios" => Ok(sqlx::query_scalar!(
            "SELECT mobile_app_url_ios FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "mobile_app_url_android" => Ok(sqlx::query_scalar!(
            "SELECT mobile_app_url_android FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "training_materials" => Ok(sqlx::query_scalar!(
            "SELECT training_materials FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "classroom_materials" => Ok(sqlx::query_scalar!(
            "SELECT classroom_materials FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "twitter_name" => Ok(sqlx::query_scalar!(
            "SELECT twitter_name FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "facebook_page" => Ok(sqlx::query_scalar!(
            "SELECT facebook_page FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "instagram_page" => Ok(sqlx::query_scalar!(
            "SELECT instagram_page FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "tags" => {
            let tags: Vec<String> = sqlx::query_scalar!(
                "SELECT tag FROM project_tags WHERE project_id = ? ORDER BY tag",
                id
            )
            .fetch_all(db())
            .await?;
            Ok(tags.join(", "))
        }
        "audiences" => {
            let auds: Vec<String> = sqlx::query_scalar!(
                "SELECT audience FROM project_audiences WHERE project_id = ? ORDER BY audience",
                id
            )
            .fetch_all(db())
            .await?;
            Ok(auds.join(", "))
        }
        // Additional fields
        "status" => Ok(sqlx::query_scalar!(
            r#"SELECT status as "s: String" FROM projects WHERE project_id = ?"#,
            id
        )
        .fetch_one(db())
        .await?),
        "difficulty" => Ok(sqlx::query_scalar!(
            "SELECT difficulty FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "search_terms" => Ok(sqlx::query_scalar!(
            "SELECT search_terms FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "signup_url" => Ok(sqlx::query_scalar!(
            "SELECT signup_url FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "opengraph_description" => Ok(sqlx::query_scalar!(
            "SELECT opengraph_description FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "begin" => {
            let v = sqlx::query_scalar!(
                r#"SELECT begin as "v: String" FROM projects WHERE project_id = ?"#,
                id
            )
            .fetch_one(db())
            .await?;
            Ok(v.unwrap_or_default())
        }
        "end" => {
            let v = sqlx::query_scalar!(
                r#"SELECT "end" as "v: String" FROM projects WHERE project_id = ?"#,
                id
            )
            .fetch_one(db())
            .await?;
            Ok(v.unwrap_or_default())
        }
        "project_timing" => Ok(sqlx::query_scalar!(
            "SELECT project_timing FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "year_started" => {
            let v =
                sqlx::query_scalar!("SELECT year_started FROM projects WHERE project_id = ?", id)
                    .fetch_one(db())
                    .await?;
            Ok(v.map(|y| y.to_string()).unwrap_or_default())
        }
        "on_demand" => {
            let v = sqlx::query_scalar!(
                r#"SELECT on_demand as "v: bool" FROM projects WHERE project_id = ?"#,
                id
            )
            .fetch_one(db())
            .await?;
            Ok(if v { "true" } else { "false" }.to_string())
        }
        "in_person" => {
            let v = sqlx::query_scalar!(
                r#"SELECT in_person as "v: bool" FROM projects WHERE project_id = ?"#,
                id
            )
            .fetch_one(db())
            .await?;
            Ok(if v { "true" } else { "false" }.to_string())
        }
        "indoors" => {
            let v = sqlx::query_scalar!(
                r#"SELECT indoors as "v: bool" FROM projects WHERE project_id = ?"#,
                id
            )
            .fetch_one(db())
            .await?;
            Ok(if v { "true" } else { "false" }.to_string())
        }
        "outdoors" => {
            let v = sqlx::query_scalar!(
                r#"SELECT outdoors as "v: bool" FROM projects WHERE project_id = ?"#,
                id
            )
            .fetch_one(db())
            .await?;
            Ok(if v { "true" } else { "false" }.to_string())
        }
        "regions_description" => Ok(sqlx::query_scalar!(
            "SELECT regions_description FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "materials" => Ok(sqlx::query_scalar!(
            "SELECT materials FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "gear" => Ok(
            sqlx::query_scalar!("SELECT gear FROM projects WHERE project_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "outcomes_label" => Ok(sqlx::query_scalar!(
            "SELECT outcomes_label FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "outcomes_link" => Ok(sqlx::query_scalar!(
            "SELECT outcomes_link FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "time_required" => Ok(sqlx::query_scalar!(
            "SELECT time_required FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "consent_statement" => Ok(sqlx::query_scalar!(
            "SELECT consent_statement FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "data_publication_url" => Ok(sqlx::query_scalar!(
            "SELECT data_publication_url FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "data_license" => Ok(sqlx::query_scalar!(
            "SELECT data_license FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "university_affiliation" => Ok(sqlx::query_scalar!(
            "SELECT university_affiliation FROM projects WHERE project_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "participation_degree" => Ok(sqlx::query_scalar!(
            r#"SELECT participation_degree as "v: String" FROM projects WHERE project_id = ?"#,
            id
        )
        .fetch_one(db())
        .await?),
        _ => Err(anyhow::anyhow!("Unknown project field: {}", field_type)),
    }
}

async fn read_event_field(id: &str, field_type: &str) -> Result<String, Error> {
    match field_type {
        "name" => Ok(
            sqlx::query_scalar!("SELECT name FROM events WHERE event_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "presenter" => Ok(sqlx::query_scalar!(
            "SELECT presenter FROM events WHERE event_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "image" => Ok(
            sqlx::query_scalar!("SELECT image FROM events WHERE event_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "image_credit" => Ok(sqlx::query_scalar!(
            "SELECT image_credit FROM events WHERE event_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "url" => Ok(
            sqlx::query_scalar!("SELECT url FROM events WHERE event_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "description" => Ok(sqlx::query_scalar!(
            "SELECT description FROM events WHERE event_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "how_to_join" => Ok(sqlx::query_scalar!(
            "SELECT how_to_join FROM events WHERE event_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "location_name" => Ok(sqlx::query_scalar!(
            "SELECT location_name FROM events WHERE event_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "location_description" => Ok(sqlx::query_scalar!(
            "SELECT location_description FROM events WHERE event_id = ?",
            id
        )
        .fetch_one(db())
        .await?),
        "address" => Ok(
            sqlx::query_scalar!("SELECT address FROM events WHERE event_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "city" => Ok(
            sqlx::query_scalar!("SELECT city FROM events WHERE event_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "state" => Ok(
            sqlx::query_scalar!("SELECT state FROM events WHERE event_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "zip" => Ok(
            sqlx::query_scalar!("SELECT zip FROM events WHERE event_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "country" => Ok(
            sqlx::query_scalar!("SELECT country FROM events WHERE event_id = ?", id)
                .fetch_one(db())
                .await?,
        ),
        "entirely_online" => {
            let v = sqlx::query_scalar!(
                r#"SELECT entirely_online as "v: bool" FROM events WHERE event_id = ?"#,
                id
            )
            .fetch_one(db())
            .await?;
            Ok(if v { "true" } else { "false" }.to_string())
        }
        "event_type" => Ok(sqlx::query_scalar!(
            r#"SELECT event_type as "v: String" FROM events WHERE event_id = ?"#,
            id
        )
        .fetch_one(db())
        .await?),
        "status" => Ok(sqlx::query_scalar!(
            r#"SELECT status as "v: String" FROM events WHERE event_id = ?"#,
            id
        )
        .fetch_one(db())
        .await?),
        _ => Err(anyhow::anyhow!("Unknown event field: {}", field_type)),
    }
}

// ---------------------------------------------------------------------------
// Parse form value into field value and store
// ---------------------------------------------------------------------------

use common::insensitivestring::InsensitiveString;
use std::collections::HashSet;

async fn save_project_field(
    project_id: Ulid,
    field_type: &str,
    values: &[String],
    source: FieldSource,
) -> Result<(), Error> {
    let value = values.first().map(|s| s.as_str()).unwrap_or("");
    let field_value = match field_type {
        "name" => ProjectFieldValue::Name(value.to_string()),
        "presenter" => ProjectFieldValue::Presenter(value.to_string()),
        "image" => ProjectFieldValue::Image(value.to_string()),
        "image_credit" => ProjectFieldValue::ImageCredit(value.to_string()),
        "url" => ProjectFieldValue::Url(value.to_string()),
        "goal" => ProjectFieldValue::Goal(value.to_string()),
        "task" => ProjectFieldValue::Task(value.to_string()),
        "description" => ProjectFieldValue::Description(value.to_string()),
        "how_to_join" => ProjectFieldValue::HowToJoin(value.to_string()),
        "location_name" => ProjectFieldValue::LocationName(value.to_string()),
        "location_description" => ProjectFieldValue::LocationDescription(value.to_string()),
        "address" => ProjectFieldValue::Address(value.to_string()),
        "city" => ProjectFieldValue::City(value.to_string()),
        "state" => ProjectFieldValue::State(value.to_string()),
        "zip" => ProjectFieldValue::Zip(value.to_string()),
        "country" => ProjectFieldValue::Country(value.to_string()),
        "entirely_online" => ProjectFieldValue::EntirelyOnline(value == "true"),
        "video_url" => ProjectFieldValue::VideoUrl(value.to_string()),
        "total_expense" => {
            let v = if value.is_empty() {
                None
            } else {
                value.parse::<f32>().ok()
            };
            ProjectFieldValue::TotalExpense(v)
        }
        "time_per_participation" => ProjectFieldValue::TimePerParticipation(value.to_string()),
        "participation_schedule" => ProjectFieldValue::ParticipationSchedule(value.to_string()),
        "mobile_app_url" => ProjectFieldValue::MobileAppUrl(value.to_string()),
        "mobile_app_url_ios" => ProjectFieldValue::MobileAppUrlIos(value.to_string()),
        "mobile_app_url_android" => ProjectFieldValue::MobileAppUrlAndroid(value.to_string()),
        "training_materials" => ProjectFieldValue::TrainingMaterials(value.to_string()),
        "classroom_materials" => ProjectFieldValue::ClassroomMaterials(value.to_string()),
        "twitter_name" => ProjectFieldValue::TwitterName(value.to_string()),
        "facebook_page" => ProjectFieldValue::FacebookPage(value.to_string()),
        "instagram_page" => ProjectFieldValue::InstagramPage(value.to_string()),
        "tags" => {
            let tags: HashSet<InsensitiveString> = value
                .split(',')
                .map(|s| InsensitiveString::from(s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect();
            ProjectFieldValue::Tags(tags)
        }
        "audiences" => {
            let auds: Box<[String]> = values
                .iter()
                .filter(|s| !s.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .into_boxed_slice();
            ProjectFieldValue::Audience(auds)
        }
        // Additional fields
        "status" => {
            let status = match value {
                "Active" => crate::project::ProjectStatus::Active,
                "PeriodicallyActive" => crate::project::ProjectStatus::PeriodicallyActive,
                "Complete" => crate::project::ProjectStatus::Complete,
                "Hiatus" => crate::project::ProjectStatus::Hiatus,
                "Pending" => crate::project::ProjectStatus::Pending,
                "Hidden" => crate::project::ProjectStatus::Hidden,
                "Expired" => crate::project::ProjectStatus::Expired,
                _ => return Err(anyhow::anyhow!("Invalid status: {}", value)),
            };
            ProjectFieldValue::Status(status)
        }
        "difficulty" => ProjectFieldValue::Difficulty(value.to_string()),
        "search_terms" => ProjectFieldValue::SearchTerms(value.to_string()),
        "signup_url" => ProjectFieldValue::SignupUrl(value.to_string()),
        "opengraph_description" => ProjectFieldValue::OpengraphDescription(value.to_string()),
        "begin" => {
            let ts = if value.is_empty() {
                None
            } else {
                Some(TimeString::try_from(value.to_string())?)
            };
            ProjectFieldValue::Begin(ts)
        }
        "end" => {
            let ts = if value.is_empty() {
                None
            } else {
                Some(TimeString::try_from(value.to_string())?)
            };
            ProjectFieldValue::End(ts)
        }
        "project_timing" => ProjectFieldValue::ProjectTiming(value.to_string()),
        "year_started" => {
            let v = if value.is_empty() {
                None
            } else {
                Some(value.parse::<i32>()?)
            };
            ProjectFieldValue::YearStarted(v)
        }
        "on_demand" => ProjectFieldValue::OnDemand(value == "true"),
        "in_person" => ProjectFieldValue::InPerson(value == "true"),
        "indoors" => ProjectFieldValue::Indoors(value == "true"),
        "outdoors" => ProjectFieldValue::Outdoors(value == "true"),
        "regions_description" => ProjectFieldValue::RegionsDescription(value.to_string()),
        "materials" => ProjectFieldValue::Materials(value.to_string()),
        "gear" => ProjectFieldValue::Gear(value.to_string()),
        "outcomes_label" => ProjectFieldValue::OutcomesLabel(value.to_string()),
        "outcomes_link" => ProjectFieldValue::OutcomesLink(value.to_string()),
        "time_required" => ProjectFieldValue::TimeRequired(value.to_string()),
        "consent_statement" => ProjectFieldValue::ConsentStatement(value.to_string()),
        "data_publication_url" => ProjectFieldValue::DataPublicationUrl(value.to_string()),
        "data_license" => ProjectFieldValue::DataLicense(value.to_string()),
        "university_affiliation" => ProjectFieldValue::UniversityAffiliation(value.to_string()),
        "participation_degree" => {
            let pd = match value {
                "CoCreated" => crate::project::ProjectParticipationDegree::CoCreated,
                "Collaborative" => crate::project::ProjectParticipationDegree::Collaborative,
                "Contributory" => crate::project::ProjectParticipationDegree::Contributory,
                "Contractual" => crate::project::ProjectParticipationDegree::Contractual,
                "Collegial" => crate::project::ProjectParticipationDegree::Collegial,
                _ => return Err(anyhow::anyhow!("Invalid participation degree: {}", value)),
            };
            ProjectFieldValue::ParticipationDegree(pd)
        }
        _ => return Err(anyhow::anyhow!("Unknown project field: {}", field_type)),
    };

    let field = ProjectField::with_source(project_id, field_value, source);
    field.store().await?;
    Ok(())
}

async fn save_event_field(
    event_id: Ulid,
    field_type: &str,
    values: &[String],
) -> Result<(), Error> {
    let value = values.first().map(|s| s.as_str()).unwrap_or("");
    let field_value = match field_type {
        "name" => EventFieldValue::Name(value.to_string()),
        "presenter" => EventFieldValue::Presenter(value.to_string()),
        "image" => EventFieldValue::Image(value.to_string()),
        "image_credit" => EventFieldValue::ImageCredit(value.to_string()),
        "url" => EventFieldValue::Url(value.to_string()),
        "description" => EventFieldValue::Description(value.to_string()),
        "how_to_join" => EventFieldValue::HowToJoin(value.to_string()),
        "location_name" => EventFieldValue::LocationName(value.to_string()),
        "location_description" => EventFieldValue::LocationDescription(value.to_string()),
        "address" => EventFieldValue::Address(value.to_string()),
        "city" => EventFieldValue::City(value.to_string()),
        "state" => EventFieldValue::State(value.to_string()),
        "zip" => EventFieldValue::Zip(value.to_string()),
        "country" => EventFieldValue::Country(value.to_string()),
        "entirely_online" => EventFieldValue::EntirelyOnline(value == "true"),
        "event_type" => EventFieldValue::EventType(
            EventType::from_storage_name(value)
                .ok_or_else(|| anyhow::anyhow!("Invalid event type: {}", value))?,
        ),
        "status" => {
            let status = match value {
                "Active" => crate::event::EventStatus::Active,
                "Cancelled" => crate::event::EventStatus::Cancelled,
                "Expired" => crate::event::EventStatus::Expired,
                _ => return Err(anyhow::anyhow!("Invalid event status: {}", value)),
            };
            EventFieldValue::Status(status)
        }
        _ => return Err(anyhow::anyhow!("Unknown event field: {}", field_type)),
    };

    let field = EventField::new(event_id, field_value);
    field.store().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

/// An optional extra field shown alongside the main field in the edit form.
#[derive(Debug, Clone)]
pub struct SupplementalField {
    pub name: String,
    pub label: String,
    pub input_kind: FieldInputKind,
    pub current_value: String,
}

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_field_edit.stpl.html")]
struct FieldEditForm {
    target_id: String,
    field_type: String,
    label: String,
    input_kind: FieldInputKind,
    current_value: String,
    supplemental: Option<SupplementalField>,
}

#[derive(TemplateOnce, Clone)]
#[template(path = "pages/detail_page_field_display.stpl.html")]
pub struct FieldDisplay {
    pub target_id: String,
    pub field_type: String,
    pub label: String,
    pub can_edit: bool,
    pub content_html: String,
    pub empty: bool,
    /// Extra classes on the `#field-X` wrapper beyond `relative` (e.g. inline layout).
    pub wrapper_extra: &'static str,
    /// Whether the edit button flows inline (for inline/icon fields) rather than
    /// being absolutely positioned in the corner.
    pub inline_button: bool,
    /// Whether to show the field label in editing mode. False for fields whose
    /// rendered content already includes a heading (ribbon boxes, the online
    /// toggle) or that are icon-only adornments.
    pub show_label: bool,
}

impl FieldDisplay {
    pub fn build(
        target_id: String,
        field_type: &str,
        label: String,
        can_edit: bool,
        content_html: String,
        empty: bool,
    ) -> Self {
        FieldDisplay {
            wrapper_extra: field_wrapper_extra(field_type),
            inline_button: field_inline_button(field_type),
            show_label: !field_self_labeled(field_type),
            target_id,
            field_type: field_type.to_string(),
            label,
            can_edit,
            content_html,
            empty,
        }
    }
}

/// Fields whose rendered content already carries its own heading (ribbon
/// boxes / the online toggle), that sit in a box the page heads with their
/// name, or that are icon-only links — for these the wrapper should not add a
/// separate label.
fn field_self_labeled(field_type: &str) -> bool {
    matches!(
        field_type,
        "goal"
            | "task"
            | "description"
            | "how_to_join"
            | "video_url"
            | "total_expense"
            | "time_per_participation"
            | "audiences"
            | "participation_schedule"
            | "training_materials"
            | "classroom_materials"
            | "entirely_online"
            | "mobile_app_url"
            | "mobile_app_url_ios"
            | "mobile_app_url_android"
            | "facebook_page"
            | "twitter_name"
            | "instagram_page"
    )
}

/// Extra wrapper classes per field, matching the original full-page layout.
fn field_wrapper_extra(field_type: &str) -> &'static str {
    match field_type {
        "city" | "state" | "zip" => "inline",
        "event_type" => "inline-block mb-2",
        "url" | "tags" => "mb-4",
        "image" => "flex flex-col max-w-full md:w-[425px]",
        _ => "",
    }
}

/// Fields whose edit button flows inline rather than being corner-positioned.
fn field_inline_button(field_type: &str) -> bool {
    matches!(
        field_type,
        "city"
            | "state"
            | "zip"
            | "mobile_app_url"
            | "mobile_app_url_ios"
            | "mobile_app_url_android"
            | "facebook_page"
            | "twitter_name"
            | "instagram_page"
    )
}

/// The editable fields shown on a detail page, in render order, per kind.
pub fn editable_field_list(kind: DetailPageKind) -> &'static [&'static str] {
    match kind {
        DetailPageKind::Project => &[
            "presenter",
            "name",
            "url",
            "goal",
            "task",
            "description",
            "entirely_online",
            "location_name",
            "regions_description",
            "location_description",
            "address",
            "city",
            "state",
            "zip",
            "country",
            "how_to_join",
            "video_url",
            "total_expense",
            "time_per_participation",
            "audiences",
            "participation_schedule",
            "mobile_app_url_android",
            "mobile_app_url_ios",
            "mobile_app_url",
            "training_materials",
            "classroom_materials",
            "facebook_page",
            "twitter_name",
            "instagram_page",
            "tags",
        ],
        DetailPageKind::Event => &[
            "presenter",
            "name",
            "event_type",
            "url",
            "description",
            "entirely_online",
            "location_name",
            "location_description",
            "address",
            "city",
            "state",
            "zip",
            "country",
            "how_to_join",
            "tags",
        ],
    }
}

/// Render every editable field's display partial from values already loaded by
/// the page (no extra queries — the page's main row SELECT already has them).
/// Both the full page and inline saves render through `format_display_value`, so
/// a saved field always matches a full page render.
pub async fn render_fields(
    target_id: &str,
    kind: DetailPageKind,
    can_edit: bool,
    values: &std::collections::HashMap<&'static str, String>,
) -> std::collections::HashMap<String, FieldDisplay> {
    let mut out = std::collections::HashMap::new();
    for &field_type in editable_field_list(kind) {
        let value = values.get(field_type).map(String::as_str).unwrap_or("");
        let content_html = format_display_value(field_type, value, target_id, kind, can_edit).await;
        let label = field_meta(field_type, kind)
            .map(|m| m.label)
            .unwrap_or("")
            .to_string();
        out.insert(
            field_type.to_string(),
            FieldDisplay::build(
                target_id.to_string(),
                field_type,
                label,
                can_edit,
                content_html,
                value.is_empty(),
            ),
        );
    }
    out
}

// ---------------------------------------------------------------------------
// Form data
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct FieldForm {
    #[serde(deserialize_with = "crate::form::deserialize_string_or_vec")]
    value: Vec<String>,
    #[serde(default)]
    image_credit: Option<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /-detail/:id/field/:field_type — returns edit form HTML
#[handler]
pub async fn get_edit_form(
    Path((id, field_type)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(|| {
        poem::Error::from_string(
            "Authentication required",
            poem::http::StatusCode::UNAUTHORIZED,
        )
    })?;

    let kind = resolve_kind(&id).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let (can_edit, _is_staff) = check_edit_permission(&id, &user, kind).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    if !can_edit {
        return Err(poem::Error::from_string(
            "Forbidden",
            poem::http::StatusCode::FORBIDDEN,
        ));
    }

    let meta = field_meta(&field_type, kind).ok_or_else(|| {
        poem::Error::from_string("Unknown field", poem::http::StatusCode::BAD_REQUEST)
    })?;

    let current_value = read_field_value(&id, &field_type, kind)
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    // For rich text fields, normalize the value to Delta JSON for the Quill editor
    let current_value = if meta.input_kind == FieldInputKind::RichText {
        common::quill::to_delta(&current_value, None).unwrap_or(current_value)
    } else {
        current_value
    };

    // Build supplemental field if applicable
    let supplemental = match field_type.as_str() {
        "image" => {
            let credit = read_field_value(&id, "image_credit", kind)
                .await
                .unwrap_or_default();
            Some(SupplementalField {
                name: "image_credit".to_string(),
                label: "Image Credit".to_string(),
                input_kind: FieldInputKind::Text,
                current_value: credit,
            })
        }
        _ => None,
    };

    let html = FieldEditForm {
        target_id: id,
        field_type,
        label: meta.label.to_string(),
        input_kind: meta.input_kind,
        current_value,
        supplemental,
    }
    .render_once()
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

/// PUT /-detail/:id/field/:field_type — save field and return display HTML
#[handler]
pub async fn save_field(
    Path((id, field_type)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
    crate::form::QsForm(form): crate::form::QsForm<FieldForm>,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(|| {
        poem::Error::from_string(
            "Authentication required",
            poem::http::StatusCode::UNAUTHORIZED,
        )
    })?;

    let kind = resolve_kind(&id).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let (can_edit, is_staff) = check_edit_permission(&id, &user, kind).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    if !can_edit {
        return Err(poem::Error::from_string(
            "Forbidden",
            poem::http::StatusCode::FORBIDDEN,
        ));
    }

    // Validate field type is known
    let meta = field_meta(&field_type, kind).ok_or_else(|| {
        poem::Error::from_string("Unknown field", poem::http::StatusCode::BAD_REQUEST)
    })?;

    let target_ulid = Ulid::from_string(&id).map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::BAD_REQUEST)
    })?;

    let source = edit_source(is_staff);

    match kind {
        DetailPageKind::Project => {
            save_project_field(target_ulid, &field_type, &form.value, source).await
        }
        DetailPageKind::Event => save_event_field(target_ulid, &field_type, &form.value).await,
    }
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    // Save supplemental fields
    if let Some(credit) = &form.image_credit {
        let credit_values = vec![credit.clone()];
        match kind {
            DetailPageKind::Project => {
                save_project_field(target_ulid, "image_credit", &credit_values, source).await
            }
            DetailPageKind::Event => {
                save_event_field(target_ulid, "image_credit", &credit_values).await
            }
        }
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;
    }

    // Log activity
    let action = edit_action(kind, source);
    let _ = log_activity(None, action, user.user_id, target_ulid, field_type.clone()).await;

    // Return updated display
    let current_value = read_field_value(&id, &field_type, kind)
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    let empty = current_value.is_empty();
    let display_html = format_display_value(&field_type, &current_value, &id, kind, true).await;

    let html = FieldDisplay::build(
        id,
        &field_type,
        meta.label.to_string(),
        true,
        display_html,
        empty,
    )
    .render_once()
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

/// GET /-detail/:id/field/:field_type/display — returns display HTML with edit marker
#[handler]
pub async fn get_display(
    Path((id, field_type)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let kind = resolve_kind(&id).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let can_edit = if let Some(ref user) = user_info {
        check_edit_permission(&id, user, kind)
            .await
            .map(|(c, _)| c)
            .unwrap_or(false)
    } else {
        false
    };

    let meta = field_meta(&field_type, kind).ok_or_else(|| {
        poem::Error::from_string("Unknown field", poem::http::StatusCode::BAD_REQUEST)
    })?;

    let current_value = read_field_value(&id, &field_type, kind)
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    let empty = current_value.is_empty();
    let display_html = format_display_value(&field_type, &current_value, &id, kind, can_edit).await;

    let html = FieldDisplay::build(
        id,
        &field_type,
        meta.label.to_string(),
        can_edit,
        display_html,
        empty,
    )
    .render_once()
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn ribbon_box(label: &str, content: impl std::fmt::Display) -> String {
    format!(
        r#"<div class="border border-linework rounded-sm bg-primary-light mb-4"><dl><dt class="ribbon-project bg-primary-dark text-white">{label}</dt><dd class="p-4">{content}</dd></dl></div>"#
    )
}

fn ribbon_subbox(label: &str, content: impl std::fmt::Display) -> String {
    format!(
        r#"<dl class="border-t border-linework bg-primary-light pb-4"><dt class="ribbon-project ribbon-subproject bg-primary-dark text-white">{label}</dt><dd class="p-4">{content}</dd></dl>"#
    )
}

fn not_set() -> &'static str {
    r#"<span class="edit-only text-gray-400 italic">(not set)</span>"#
}

/// Format a field value for display as HTML
async fn format_image_display(target_id: &str, value: &str, kind: DetailPageKind) -> String {
    if value.is_empty() {
        return r#"<div class="rounded-sm border border-linework bg-gray-100 h-48 flex items-center justify-center text-gray-400 italic">(not set)</div>"#.to_string();
    }
    let credit = read_field_value(target_id, "image_credit", kind)
        .await
        .unwrap_or_default();
    let mut html = format!(
        r#"<img class="rounded-sm border border-linework" src="{}" alt="">"#,
        html_escape(value)
    );
    if !credit.is_empty() {
        html.push_str(&format!(
            r#"<span class="block text-center text-xs text-[#ccc]">{}</span>"#,
            html_escape(&credit)
        ));
    }
    html
}

// SVG glyphs reused from the full-page detail template, kept as constants so the
// inline-edit partial and the page render byte-identical markup.
const SVG_LINK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 512" class="fill-primary-action w-4"><path d="M579.8 267.7c56.5-56.5 56.5-148 0-204.5c-50-50-128.8-56.5-186.3-15.4l-1.6 1.1c-14.4 10.3-17.7 30.3-7.4 44.6s30.3 17.7 44.6 7.4l1.6-1.1c32.1-22.9 76-19.3 103.8 8.6c31.5 31.5 31.5 82.5 0 114L422.3 334.8c-31.5 31.5-82.5 31.5-114 0c-27.9-27.9-31.5-71.8-8.6-103.8l1.1-1.6c10.3-14.4 6.9-34.4-7.4-44.6s-34.4-6.9-44.6 7.4l-1.1 1.6C206.5 251.2 213 330 263 380c56.5 56.5 148 56.5 204.5 0L579.8 267.7zM60.2 244.3c-56.5 56.5-56.5 148 0 204.5c50 50 128.8 56.5 186.3 15.4l1.6-1.1c14.4-10.3 17.7-30.3 7.4-44.6s-30.3-17.7-44.6-7.4l-1.6 1.1c-32.1 22.9-76 19.3-103.8-8.6C74 372 74 321 105.5 289.5L217.7 177.2c31.5-31.5 82.5-31.5 114 0c27.9 27.9 31.5 71.8 8.6 103.9l-1.1 1.6c-10.3 14.4-6.9 34.4 7.4 44.6s34.4 6.9 44.6-7.4l1.1-1.6C433.5 260.8 427 182 377 132c-56.5-56.5-148-56.5-204.5 0L60.2 244.3z"/></svg>"##;
const SVG_ANDROID: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 299.679 299.679" class="fill-primary-action h-8"><path d="M181.122 299.679c10.02 0 18.758-8.738 18.758-18.758v-43.808h12.525c7.516 0 12.525-5.011 12.525-12.525V99.466H74.749v125.123c0 7.515 5.01 12.525 12.525 12.525H99.8v43.808c0 10.02 8.736 18.758 18.758 18.758 10.019 0 18.756-8.738 18.756-18.758v-43.808h25.051v43.808c-.001 10.019 8.737 18.757 18.757 18.757z"/></svg>"##;
const SVG_IOS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 384 512" class="h-8 fill-primary-action"><path d="M318.7 268.7c-.2-36.7 16.4-64.4 50-84.8-18.8-26.9-47.2-41.7-84.7-44.6-35.5-2.8-74.3 20.7-88.5 20.7-15 0-49.4-19.7-76.4-19.7C63.3 141.2 4 184.8 4 273.5q0 39.3 14.4 81.2c12.8 36.7 59 126.7 107.2 125.2 25.2-.6 43-17.9 75.8-17.9 31.8 0 48.3 17.9 76.4 17.9 48.6-.7 90.4-82.5 102.6-119.3-65.2-30.7-61.7-90-61.7-91.9zm-56.6-164.2c27.3-32.4 24.8-61.9 24-72.5-24.1 1.4-52 16.4-67.9 34.9-17.5 19.8-27.8 44.3-25.6 71.9 26.1 2 49.9-11.4 69.5-34.3z"/></svg>"##;
const SVG_FACEBOOK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" class="fill-primary-action h-8"><path d="M64 32C28.7 32 0 60.7 0 96V416c0 35.3 28.7 64 64 64h98.2V334.2H109.4V256h52.8V222.3c0-87.1 39.4-127.5 125-127.5c16.2 0 44.2 3.2 55.7 6.4V172c-6-.6-16.5-1-29.6-1c-42 0-58.2 15.9-58.2 57.2V256h83.6l-14.4 78.2H255V480H384c35.3 0 64-28.7 64-64V96c0-35.3-28.7-64-64-64H64z"/></svg>"##;
const SVG_TWITTER: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 448 512" class="fill-primary-action h-8"><path d="M64 32C28.7 32 0 60.7 0 96V416c0 35.3 28.7 64 64 64H384c35.3 0 64-28.7 64-64V96c0-35.3-28.7-64-64-64H64zm297.1 84L257.3 234.6 379.4 396H283.8L209 298.1 123.3 396H75.8l111-126.9L69.7 116h98l67.7 89.5L313.6 116h47.5zM323.3 367.6L153.4 142.9H125.1L296.9 367.6h26.3z"/></svg>"##;
const SVG_INSTAGRAM: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 448 512" class="fill-primary-action h-8"><path d="M194.4 211.7a53.3 53.3 0 1 0 59.3 88.7 53.3 53.3 0 1 0 -59.3-88.7zm142.3-68.4c-5.2-5.2-11.5-9.3-18.4-12c-18.1-7.1-57.6-6.8-83.1-6.5c-4.1 0-7.9 .1-11.2 .1c-3.3 0-7.2 0-11.4-.1c-25.5-.3-64.8-.7-82.9 6.5c-6.9 2.7-13.1 6.8-18.4 12s-9.3 11.5-12 18.4c-7.1 18.1-6.7 57.7-6.5 83.2c0 4.1 .1 7.9 .1 11.1s0 7-.1 11.1c-.2 25.5-.6 65.1 6.5 83.2c2.7 6.9 6.8 13.1 12 18.4s11.5 9.3 18.4 12c18.1 7.1 57.6 6.8 83.1 6.5c4.1 0 7.9-.1 11.2-.1c3.3 0 7.2 0 11.4 .1c25.5 .3 64.8 .7 82.9-6.5c6.9-2.7 13.1-6.8 18.4-12s9.3-11.5 12-18.4c7.2-18 6.8-57.4 6.5-83c0-4.2-.1-8.1-.1-11.4s0-7.1 .1-11.4c.3-25.5 .7-64.9-6.5-83l0 0c-2.7-6.9-6.8-13.1-12-18.4zM448 96c0-35.3-28.7-64-64-64H64C28.7 32 0 60.7 0 96V416c0 35.3 28.7 64 64 64H384c35.3 0 64-28.7 64-64V96zM357 389c-18.7 18.7-41.4 24.6-67 25.9c-26.4 1.5-105.6 1.5-132 0c-25.6-1.3-48.3-7.2-67-25.9s-24.6-41.4-25.8-67c-1.5-26.4-1.5-105.6 0-132c1.3-25.6 7.1-48.3 25.8-67s41.5-24.6 67-25.8c26.4-1.5 105.6-1.5 132 0c25.6 1.3 48.3 7.1 67 25.8s24.6 41.4 25.8 67c1.5 26.3 1.5 105.4 0 131.9c-1.3 25.6-7.1 48.3-25.8 67z"/></svg>"##;

/// Render the inner content HTML for a field — everything inside the `#field-X`
/// wrapper, excluding the wrapper div and edit button. This is the single source
/// of truth for field display: both the full page (via `DetailPageContent::field`)
/// and the inline-edit partial (`save_field`/`get_display`) render through it, so
/// a saved value always matches a full page render.
async fn format_display_value(
    field_type: &str,
    value: &str,
    target_id: &str,
    kind: DetailPageKind,
    can_edit: bool,
) -> String {
    let empty = value.is_empty();
    let esc = || html_escape(value);

    match field_type {
        "image" => format_image_display(target_id, value, kind).await,

        // Hero
        "presenter" => {
            if empty {
                format!(
                    r#"<h2 class="font-condensed uppercase text-sm tracking-wider m-0">{}</h2>"#,
                    not_set()
                )
            } else {
                format!(
                    r#"<h2 class="font-condensed uppercase text-accent1 text-sm tracking-wider m-0">{}</h2>"#,
                    esc()
                )
            }
        }
        "name" => format!(
            r#"<h1 class="text-primary-dark text-xl font-semibold leading-tight">{}</h1>"#,
            esc()
        ),
        "event_type" => {
            if !empty {
                format!(r#"<span class="btn btn-xs btn-secondary">{}</span>"#, esc())
            } else if can_edit {
                not_set().to_string()
            } else {
                String::new()
            }
        }
        "url" => {
            if empty {
                not_set().to_string()
            } else {
                format!(
                    r#"<a href="{0}" class="text-primary-action flex gap-2" target="_blank" rel="noopener">{1}<span class="underline">{0}</span></a>"#,
                    esc(),
                    SVG_LINK
                )
            }
        }

        // Ribbon-box fields (top-level styled blocks)
        "goal" if kind == DetailPageKind::Project => {
            ribbon_box("Goal", if empty { not_set().to_string() } else { esc() })
        }
        "task" if kind == DetailPageKind::Project => {
            ribbon_box("Task", if empty { not_set().to_string() } else { esc() })
        }
        "description" => {
            if empty {
                ribbon_box("Description", not_set())
            } else {
                let html = common::quill::content_to_html(value)
                    .map(|(h, _)| h)
                    .unwrap_or_else(|_| value.to_string());
                ribbon_box("Description", &html)
            }
        }
        // Inside the page's "How to Get Started" box, which also carries the
        // guidance the page adds ahead of the project's own instructions.
        "how_to_join" => {
            if empty {
                not_set().to_string()
            } else {
                common::quill::content_to_html(value)
                    .map(|(h, _)| h)
                    .unwrap_or_else(|_| value.to_string())
            }
        }

        // "Where" box interior — booleans/location/address
        "entirely_online" => {
            if value == "true" {
                r#"<span class="text-sm">Entirely Online: Yes</span>"#.to_string()
            } else if can_edit {
                r#"<span class="text-sm"><span class="edit-only text-gray-400 italic">Entirely Online: No</span></span>"#.to_string()
            } else {
                r#"<span class="text-sm"></span>"#.to_string()
            }
        }
        "location_name" => {
            if !empty {
                format!(r#"<p class="mb-1 font-semibold">{}</p>"#, esc())
            } else if can_edit {
                format!(r#"<p class="mb-1 font-semibold">{}</p>"#, not_set())
            } else {
                String::new()
            }
        }
        "regions_description" => {
            if !empty {
                format!(r#"<p class="mb-1">{}</p>"#, esc())
            } else if can_edit {
                format!(r#"<p class="mb-1">{}</p>"#, not_set())
            } else {
                String::new()
            }
        }
        "location_description" => {
            if !empty {
                format!(r#"<p class="mb-1">{}</p>"#, esc())
            } else if can_edit {
                format!(r#"<p class="mb-1">{}</p>"#, not_set())
            } else {
                String::new()
            }
        }
        "address" => {
            if !empty {
                format!(r#"<p class="mb-1">{}</p>"#, esc())
            } else if can_edit {
                format!(r#"<p class="mb-1">{}</p>"#, not_set())
            } else {
                String::new()
            }
        }
        "city" | "state" | "zip" => {
            if !empty {
                esc()
            } else if can_edit {
                not_set().to_string()
            } else {
                String::new()
            }
        }
        "country" => {
            if !empty {
                format!(r#"<p class="mb-2">{}</p>"#, esc())
            } else if can_edit {
                format!(r#"<p class="mb-2">{}</p>"#, not_set())
            } else {
                String::new()
            }
        }

        // More Info accordion interior fields (sub-boxes)
        "video_url" => ribbon_subbox(
            "Video",
            if empty {
                not_set().to_string()
            } else {
                format!(
                    r#"<a href="{}" class="text-primary-action hover:underline" target="_blank" rel="noopener">Check out the how-to video</a>"#,
                    esc()
                )
            },
        ),
        "total_expense" => {
            let content = if empty {
                not_set().to_string()
            } else if let Ok(v) = value.parse::<f64>() {
                if v == 0.0 {
                    "Free".to_string()
                } else {
                    format!("${:.2}", v)
                }
            } else {
                esc()
            };
            ribbon_subbox("Total Expense", content)
        }
        "time_per_participation" => ribbon_subbox(
            "Spend the Time",
            if empty { not_set().to_string() } else { esc() },
        ),
        "audiences" => {
            let content = if empty {
                not_set().to_string()
            } else {
                value
                    .split(", ")
                    .map(html_escape)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            ribbon_subbox("Ideal Age Group(s)", content)
        }
        "participation_schedule" => ribbon_subbox(
            "Ideal Frequency",
            if empty { not_set().to_string() } else { esc() },
        ),
        "training_materials" => ribbon_subbox(
            "Training Materials",
            if empty {
                not_set().to_string()
            } else {
                format!(
                    r#"<a href="{0}" class="text-primary-action hover:underline" target="_blank" rel="noopener">{0}</a>"#,
                    esc()
                )
            },
        ),
        "classroom_materials" => ribbon_subbox(
            "Classroom Materials",
            if empty {
                not_set().to_string()
            } else {
                format!(
                    r#"<a href="{0}" class="text-primary-action hover:underline" target="_blank" rel="noopener">{0}</a>"#,
                    esc()
                )
            },
        ),

        // App links (inside the "Apps" sub-box) — just the icon link
        "mobile_app_url_android" => {
            if empty {
                String::new()
            } else {
                format!(
                    r#"<a href="{}" class="text-primary-action" target="_blank" rel="noopener">{}</a>"#,
                    esc(),
                    SVG_ANDROID
                )
            }
        }
        "mobile_app_url_ios" => {
            if empty {
                String::new()
            } else {
                format!(
                    r#"<a href="{}" class="text-primary-action" target="_blank" rel="noopener">{}</a>"#,
                    esc(),
                    SVG_IOS
                )
            }
        }
        "mobile_app_url" => {
            if empty {
                String::new()
            } else {
                format!(
                    r#"<a href="{}" class="text-primary-action hover:underline" target="_blank" rel="noopener">Mobile App</a>"#,
                    esc()
                )
            }
        }

        // Social links (inside the "Social Media" sub-box) — just the icon link
        "facebook_page" => {
            if empty {
                String::new()
            } else {
                format!(
                    r#"<a href="https://facebook.com/{}" target="_blank" rel="noopener">{}</a>"#,
                    esc(),
                    SVG_FACEBOOK
                )
            }
        }
        "twitter_name" => {
            if empty {
                String::new()
            } else {
                format!(
                    r#"<a href="https://twitter.com/{}" target="_blank" rel="noopener">{}</a>"#,
                    esc(),
                    SVG_TWITTER
                )
            }
        }
        "instagram_page" => {
            if empty {
                String::new()
            } else {
                format!(
                    r#"<a href="https://instagram.com/{}" target="_blank" rel="noopener">{}</a>"#,
                    esc(),
                    SVG_INSTAGRAM
                )
            }
        }

        // Tags — chips linking to search
        "tags" => {
            if empty {
                not_set().to_string()
            } else {
                let chips = value
                    .split(", ")
                    .map(|t| {
                        format!(
                            r#"<a href="/-search?phrase[]={0}" class="btn btn-xs btn-secondary">{0}</a>"#,
                            html_escape(t)
                        )
                    })
                    .collect::<String>();
                format!(r#"<div class="flex flex-wrap gap-2">{chips}</div>"#)
            }
        }

        // Default: escaped text
        _ => {
            if empty {
                not_set().to_string()
            } else {
                esc()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Event instance management
// ---------------------------------------------------------------------------

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_instances.stpl.html")]
pub struct InstancesDisplay {
    pub target_id: String,
    pub instances: Vec<super::detail_page::EventInstanceDisplay>,
    pub can_edit: bool,
}

#[derive(serde::Deserialize)]
struct InstanceForm {
    begin: String,
    end: String,
}

/// GET /-detail/:id/instances — full instances list
#[handler]
pub async fn get_instances_display(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let can_edit = if let Some(ref user) = user_info {
        check_edit_permission(&id, user, DetailPageKind::Event)
            .await
            .map(|(c, _)| c)
            .unwrap_or(false)
    } else {
        false
    };

    let instances = load_instances(&id).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let html = InstancesDisplay {
        target_id: id,
        instances,
        can_edit,
    }
    .render_once()
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

/// POST /-detail/:id/instances — add a new instance
#[handler]
pub async fn add_instance(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
    poem::web::Form(form): poem::web::Form<InstanceForm>,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(|| {
        poem::Error::from_string(
            "Authentication required",
            poem::http::StatusCode::UNAUTHORIZED,
        )
    })?;

    let (can_edit, is_staff) = check_edit_permission(&id, &user, DetailPageKind::Event)
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    if !can_edit {
        return Err(poem::Error::from_string(
            "Forbidden",
            poem::http::StatusCode::FORBIDDEN,
        ));
    }

    let begin = TimeString::try_from(form.begin).map_err(|e| {
        poem::Error::from_string(
            format!("Invalid begin: {}", e),
            poem::http::StatusCode::BAD_REQUEST,
        )
    })?;
    let end = TimeString::try_from(form.end).map_err(|e| {
        poem::Error::from_string(
            format!("Invalid end: {}", e),
            poem::http::StatusCode::BAD_REQUEST,
        )
    })?;

    let begin_str: String = begin.into();
    let end_str: String = end.into();

    sqlx::query!(
        r#"INSERT INTO event_instances (event_id, "begin", "end") VALUES (?, ?, ?)"#,
        id,
        begin_str,
        end_str,
    )
    .execute(db())
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    // Log activity
    let target_ulid = Ulid::from_string(&id).map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::BAD_REQUEST)
    })?;
    let _ = log_activity(
        None,
        record_edit(DetailPageKind::Event, is_staff),
        user.user_id,
        target_ulid,
        "instances".to_string(),
    )
    .await;

    // Return updated display
    let instances = load_instances(&id).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let html = InstancesDisplay {
        target_id: id,
        instances,
        can_edit: true,
    }
    .render_once()
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

/// PUT /-detail/:id/instances/:index — update an instance
#[handler]
pub async fn update_instance(
    Path((id, index)): Path<(String, usize)>,
    Authenticated(user_info): Authenticated,
    poem::web::Form(form): poem::web::Form<InstanceForm>,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(|| {
        poem::Error::from_string(
            "Authentication required",
            poem::http::StatusCode::UNAUTHORIZED,
        )
    })?;

    let (can_edit, is_staff) = check_edit_permission(&id, &user, DetailPageKind::Event)
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    if !can_edit {
        return Err(poem::Error::from_string(
            "Forbidden",
            poem::http::StatusCode::FORBIDDEN,
        ));
    }

    let new_begin = TimeString::try_from(form.begin).map_err(|e| {
        poem::Error::from_string(
            format!("Invalid begin: {}", e),
            poem::http::StatusCode::BAD_REQUEST,
        )
    })?;
    let new_end = TimeString::try_from(form.end).map_err(|e| {
        poem::Error::from_string(
            format!("Invalid end: {}", e),
            poem::http::StatusCode::BAD_REQUEST,
        )
    })?;

    // Read all instances, update the one at index, rewrite all
    let rows = sqlx::query!(
        r#"SELECT "begin", "end" FROM event_instances WHERE event_id = ? ORDER BY "begin""#,
        id
    )
    .fetch_all(db())
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    if index >= rows.len() {
        return Err(poem::Error::from_string(
            "Instance not found",
            poem::http::StatusCode::NOT_FOUND,
        ));
    }

    // Delete all and re-insert with the update applied
    sqlx::query!("DELETE FROM event_instances WHERE event_id = ?", id)
        .execute(db())
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    for (i, row) in rows.iter().enumerate() {
        let (b, e) = if i == index {
            let b: String = new_begin.clone().into();
            let e: String = new_end.clone().into();
            (b, e)
        } else {
            (row.begin.clone(), row.end.clone())
        };
        sqlx::query!(
            r#"INSERT INTO event_instances (event_id, "begin", "end") VALUES (?, ?, ?)"#,
            id,
            b,
            e,
        )
        .execute(db())
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;
    }

    let target_ulid = Ulid::from_string(&id).map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::BAD_REQUEST)
    })?;
    let _ = log_activity(
        None,
        record_edit(DetailPageKind::Event, is_staff),
        user.user_id,
        target_ulid,
        "instances".to_string(),
    )
    .await;

    let instances = load_instances(&id).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let html = InstancesDisplay {
        target_id: id,
        instances,
        can_edit: true,
    }
    .render_once()
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

/// DELETE /-detail/:id/instances/:index — remove an instance
#[handler]
pub async fn delete_instance(
    Path((id, index)): Path<(String, usize)>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(|| {
        poem::Error::from_string(
            "Authentication required",
            poem::http::StatusCode::UNAUTHORIZED,
        )
    })?;

    let (can_edit, is_staff) = check_edit_permission(&id, &user, DetailPageKind::Event)
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    if !can_edit {
        return Err(poem::Error::from_string(
            "Forbidden",
            poem::http::StatusCode::FORBIDDEN,
        ));
    }

    let rows = sqlx::query!(
        r#"SELECT "begin", "end" FROM event_instances WHERE event_id = ? ORDER BY "begin""#,
        id
    )
    .fetch_all(db())
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    if index >= rows.len() {
        return Err(poem::Error::from_string(
            "Instance not found",
            poem::http::StatusCode::NOT_FOUND,
        ));
    }

    // Delete all and re-insert without the deleted one
    sqlx::query!("DELETE FROM event_instances WHERE event_id = ?", id)
        .execute(db())
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    for (i, row) in rows.iter().enumerate() {
        if i == index {
            continue;
        }
        sqlx::query!(
            r#"INSERT INTO event_instances (event_id, "begin", "end") VALUES (?, ?, ?)"#,
            id,
            row.begin,
            row.end,
        )
        .execute(db())
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;
    }

    let target_ulid = Ulid::from_string(&id).map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::BAD_REQUEST)
    })?;
    let _ = log_activity(
        None,
        record_edit(DetailPageKind::Event, is_staff),
        user.user_id,
        target_ulid,
        "instances".to_string(),
    )
    .await;

    let instances = load_instances(&id).await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let html = InstancesDisplay {
        target_id: id,
        instances,
        can_edit: true,
    }
    .render_once()
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

/// Load event instances for display
async fn load_instances(
    event_id: &str,
) -> Result<Vec<super::detail_page::EventInstanceDisplay>, Error> {
    let rows = sqlx::query!(
        r#"SELECT "begin", "end" FROM event_instances WHERE event_id = ? ORDER BY "begin""#,
        event_id
    )
    .fetch_all(db())
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let begin = TimeString::try_from(r.begin).ok()?;
            let end = TimeString::try_from(r.end).ok()?;
            Some(super::detail_page::EventInstanceDisplay { begin, end })
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Organizer contacts (projects and events): private (editors-only) inline
// editor. Contact info is never sent to the public — the card is rendered only
// when can_edit, and changes peer through {Project,Event}Field::Contacts.
// The kind is resolved from the target id, so one implementation serves both.
// ---------------------------------------------------------------------------

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_contacts.stpl.html")]
pub struct ContactsDisplay {
    pub target_id: String,
    pub contacts: Vec<super::detail_page::ContactDisplay>,
    pub can_edit: bool,
}

#[derive(serde::Deserialize)]
struct ContactForm {
    #[serde(default)]
    role: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    affiliation: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    phone: String,
    #[serde(default)]
    address: String,
}

fn parse_contact_role(role: &str) -> ContactRole {
    match role {
        "Manager" => ContactRole::Manager,
        "ContactPerson" => ContactRole::ContactPerson,
        "Technical" => ContactRole::Technical,
        "Notification" => ContactRole::Notification,
        _ => ContactRole::ContactPerson,
    }
}

fn contact_role_str(role: ContactRole) -> &'static str {
    match role {
        ContactRole::Manager => "Manager",
        ContactRole::ContactPerson => "ContactPerson",
        ContactRole::Technical => "Technical",
        ContactRole::Notification => "Notification",
    }
}

fn trimmed_option(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn contacts_to_display(contacts: &[Contact]) -> Vec<super::detail_page::ContactDisplay> {
    contacts
        .iter()
        .map(|c| super::detail_page::ContactDisplay {
            contact_id: c.contact_id.to_string(),
            name: c.name.clone().unwrap_or_default(),
            affiliation: c.affiliation.clone().unwrap_or_default(),
            email: c.email.clone(),
            phone: c.phone.clone().unwrap_or_default(),
        })
        .collect()
}

// --- Shared helpers used by both the contacts and managers editors ---------

/// Map any displayable error to a 500.
fn internal_error<E: std::fmt::Display>(e: E) -> poem::Error {
    poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
}

/// Resolve the target's kind, confirm the user may edit it, and parse its id.
/// Rejects unauthenticated callers (handled by the caller's `ok_or`) and
/// non-editors (403). Works for both projects and events.
async fn require_editor(
    id: &str,
    user: &BasicInfo,
) -> poem::Result<(DetailPageKind, Ulid, FieldSource)> {
    let kind = resolve_kind(id).await.map_err(internal_error)?;
    let (can_edit, is_staff) = check_edit_permission(id, user, kind)
        .await
        .map_err(internal_error)?;
    if !can_edit {
        return Err(poem::Error::from_string(
            "Forbidden",
            poem::http::StatusCode::FORBIDDEN,
        ));
    }
    let target = Ulid::from_string(id).map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::BAD_REQUEST)
    })?;
    Ok((kind, target, edit_source(is_staff)))
}

/// The source an edit on the page is written from: staff edits outrank the
/// project's own people's, and both outrank imports.
fn edit_source(is_staff: bool) -> FieldSource {
    if is_staff {
        FieldSource::Staff
    } else {
        FieldSource::Form
    }
}

/// The activity-log action for an edit of the given kind, written from
/// `source`.
fn edit_action(kind: DetailPageKind, source: FieldSource) -> &'static str {
    record_edit(kind, source == FieldSource::Staff)
}

fn auth_required() -> poem::Error {
    poem::Error::from_string(
        "Authentication required",
        poem::http::StatusCode::UNAUTHORIZED,
    )
}

/// Load the current contact list via the peered field for either kind.
async fn load_contacts(kind: DetailPageKind, target: Ulid) -> Result<Vec<Contact>, Error> {
    match kind {
        DetailPageKind::Event => {
            match EventField::load((target, EventFieldType::Contacts))
                .await?
                .field
            {
                EventFieldValue::Contacts(contacts) => Ok(contacts.into_vec()),
                _ => Ok(Vec::new()),
            }
        }
        DetailPageKind::Project => {
            match ProjectField::load((target, ProjectFieldType::Contacts))
                .await?
                .field
            {
                ProjectFieldValue::Contacts(contacts) => Ok(contacts.into_vec()),
                _ => Ok(Vec::new()),
            }
        }
    }
}

/// Persist a contact list via the peered field for either kind.
async fn store_contacts(
    kind: DetailPageKind,
    target: Ulid,
    contacts: &[Contact],
    source: FieldSource,
) -> Result<(), Error> {
    let boxed = contacts.to_vec().into_boxed_slice();
    match kind {
        DetailPageKind::Event => {
            EventField::new(target, EventFieldValue::Contacts(boxed))
                .store()
                .await
        }
        DetailPageKind::Project => {
            ProjectField::with_source(target, ProjectFieldValue::Contacts(boxed), source)
                .store()
                .await
        }
    }
}

fn contacts_response(target_id: String, contacts: &[Contact]) -> poem::Result<poem::Response> {
    let html = ContactsDisplay {
        target_id,
        contacts: contacts_to_display(contacts),
        can_edit: true,
    }
    .render_once()
    .map_err(internal_error)?;

    Ok(Html(html).into_response())
}

/// POST /-detail/:id/contacts — add an organizer contact (editors only).
#[handler]
pub async fn add_contact(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
    poem::web::Form(form): poem::web::Form<ContactForm>,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(auth_required)?;
    let (kind, target, source) = require_editor(&id, &user).await?;

    let email = form.email.trim();
    if email.is_empty() {
        return Err(poem::Error::from_string(
            "A contact email is required.",
            poem::http::StatusCode::BAD_REQUEST,
        ));
    }

    let mut contacts = load_contacts(kind, target).await.map_err(internal_error)?;
    contacts.push(Contact {
        contact_id: Ulid::new(),
        role: parse_contact_role(&form.role),
        name: trimmed_option(&form.name),
        affiliation: trimmed_option(&form.affiliation),
        email: email.to_string(),
        phone: trimmed_option(&form.phone),
        address: trimmed_option(&form.address),
    });

    store_contacts(kind, target, &contacts, source)
        .await
        .map_err(internal_error)?;

    let _ = log_activity(
        None,
        edit_action(kind, source),
        user.user_id,
        target,
        "contacts".to_string(),
    )
    .await;

    contacts_response(id, &contacts)
}

/// DELETE /-detail/:id/contacts/:contact_id — remove an organizer contact.
#[handler]
pub async fn delete_contact(
    Path((id, contact_id)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(auth_required)?;
    let (kind, target, source) = require_editor(&id, &user).await?;

    let contacts: Vec<Contact> = load_contacts(kind, target)
        .await
        .map_err(internal_error)?
        .into_iter()
        .filter(|c| c.contact_id.to_string() != contact_id)
        .collect();

    store_contacts(kind, target, &contacts, source)
        .await
        .map_err(internal_error)?;

    let _ = log_activity(
        None,
        edit_action(kind, source),
        user.user_id,
        target,
        "contacts".to_string(),
    )
    .await;

    contacts_response(id, &contacts)
}

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_contact_edit.stpl.html")]
pub struct ContactEditForm {
    pub target_id: String,
    pub contact_id: String,
    pub role: String,
    pub name: String,
    pub affiliation: String,
    pub email: String,
    pub phone: String,
    pub address: String,
}

/// GET /-detail/:id/contacts — full contacts card (used to refresh / cancel an edit).
#[handler]
pub async fn get_contacts_display(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(auth_required)?;
    let (kind, target, _) = require_editor(&id, &user).await?;
    let contacts = load_contacts(kind, target).await.map_err(internal_error)?;
    contacts_response(id, &contacts)
}

/// GET /-detail/:id/contacts/:contact_id/edit — inline edit form for one contact.
#[handler]
pub async fn get_contact_edit_form(
    Path((id, contact_id)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(auth_required)?;
    let (kind, target, _) = require_editor(&id, &user).await?;

    let contact = load_contacts(kind, target)
        .await
        .map_err(internal_error)?
        .into_iter()
        .find(|c| c.contact_id.to_string() == contact_id)
        .ok_or_else(|| {
            poem::Error::from_string("Contact not found", poem::http::StatusCode::NOT_FOUND)
        })?;

    let html = ContactEditForm {
        target_id: id,
        contact_id,
        role: contact_role_str(contact.role).to_string(),
        name: contact.name.unwrap_or_default(),
        affiliation: contact.affiliation.unwrap_or_default(),
        email: contact.email,
        phone: contact.phone.unwrap_or_default(),
        address: contact.address.unwrap_or_default(),
    }
    .render_once()
    .map_err(internal_error)?;
    Ok(Html(html).into_response())
}

/// PUT /-detail/:id/contacts/:contact_id — save edits to a contact.
#[handler]
pub async fn update_contact(
    Path((id, contact_id)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
    poem::web::Form(form): poem::web::Form<ContactForm>,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(auth_required)?;
    let (kind, target, source) = require_editor(&id, &user).await?;

    let email = form.email.trim();
    if email.is_empty() {
        return Err(poem::Error::from_string(
            "A contact email is required.",
            poem::http::StatusCode::BAD_REQUEST,
        ));
    }

    let mut contacts = load_contacts(kind, target).await.map_err(internal_error)?;

    let found = contacts
        .iter_mut()
        .find(|c| c.contact_id.to_string() == contact_id);
    let Some(contact) = found else {
        return Err(poem::Error::from_string(
            "Contact not found",
            poem::http::StatusCode::NOT_FOUND,
        ));
    };
    contact.role = parse_contact_role(&form.role);
    contact.name = trimmed_option(&form.name);
    contact.affiliation = trimmed_option(&form.affiliation);
    contact.email = email.to_string();
    contact.phone = trimmed_option(&form.phone);
    contact.address = trimmed_option(&form.address);

    store_contacts(kind, target, &contacts, source)
        .await
        .map_err(internal_error)?;

    let _ = log_activity(
        None,
        edit_action(kind, source),
        user.user_id,
        target,
        "contacts".to_string(),
    )
    .await;

    contacts_response(id, &contacts)
}

// ---------------------------------------------------------------------------
// Facebook promotion (events only): what the organizer said about promoting
// the event on Facebook, private to editors. The admin event page shows the
// same card, so staff read and change it in one place. Changes peer through
// EventField::FacebookEvent.
// ---------------------------------------------------------------------------

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_facebook.stpl.html")]
pub struct FacebookEventDisplay {
    pub target_id: String,
    /// The answers as stored.
    pub facebook_event: FacebookEvent,
    /// A change that was refused, kept open in the form to be corrected.
    pub refused: Option<RefusedFacebookAnswers>,
}

pub struct RefusedFacebookAnswers {
    pub answers: FacebookEvent,
    pub error: FormError,
}

impl FacebookEventDisplay {
    /// The answers the form starts from: a refused change, or the stored
    /// answers.
    fn form_answers(&self) -> &FacebookEvent {
        self.refused
            .as_ref()
            .map_or(&self.facebook_event, |refused| &refused.answers)
    }
}

/// An event's Facebook answers, via the peered field.
async fn load_facebook_event(event_id: Ulid) -> Result<FacebookEvent, Error> {
    match EventField::load((event_id, EventFieldType::FacebookEvent))
        .await?
        .field
    {
        EventFieldValue::FacebookEvent(facebook_event) => Ok(facebook_event),
        _ => Ok(FacebookEvent::default()),
    }
}

/// PUT /-detail/:id/facebook — record the organizer's Facebook answers
/// (events only, editors only).
#[handler]
pub async fn update_facebook_event(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
    poem::web::Form(form): poem::web::Form<FacebookAnswers>,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(auth_required)?;
    let (kind, target, source) = require_editor(&id, &user).await?;
    if kind != DetailPageKind::Event {
        return Err(poem::Error::from_string(
            "Only events have Facebook answers",
            poem::http::StatusCode::BAD_REQUEST,
        ));
    }

    let answers = form.facebook_event();

    let display = if let Some(problem) = answers.problem() {
        FacebookEventDisplay {
            target_id: id,
            facebook_event: load_facebook_event(target).await.map_err(internal_error)?,
            refused: Some(RefusedFacebookAnswers {
                answers,
                error: FormError::at(facebook_questions::URL_FIELD, problem),
            }),
        }
    } else {
        EventField::new(target, EventFieldValue::FacebookEvent(answers.clone()))
            .store()
            .await
            .map_err(internal_error)?;
        let _ = log_activity(
            None,
            edit_action(kind, source),
            user.user_id,
            target,
            "facebook_event".to_string(),
        )
        .await;
        FacebookEventDisplay {
            target_id: id,
            facebook_event: answers,
            refused: None,
        }
    };

    let html = display.render_once().map_err(internal_error)?;
    Ok(Html(html).into_response())
}

// ---------------------------------------------------------------------------
// Page managers (non-staff editors): private (editors-only) inline editor,
// mirroring contacts. Changes peer through {Project,Event}Field::PageEditorIds.
// ---------------------------------------------------------------------------

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_managers.stpl.html")]
pub struct ManagersDisplay {
    pub target_id: String,
    pub managers: Vec<super::detail_page::ManagerDisplay>,
    pub can_edit: bool,
    pub error: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_manager_edit.stpl.html")]
pub struct ManagerEditForm {
    pub target_id: String,
    pub user_id: String,
    pub email: String,
}

#[derive(serde::Deserialize)]
struct ManagerForm {
    #[serde(default)]
    email: String,
}

/// Load the current manager (page-editor) ids via the peered field for either kind.
async fn load_manager_ids(kind: DetailPageKind, target: Ulid) -> Result<Vec<Ulid>, Error> {
    match kind {
        DetailPageKind::Event => {
            match EventField::load((target, EventFieldType::PageEditorIds))
                .await?
                .field
            {
                EventFieldValue::PageEditorIds(ids) => Ok(ids.into_vec()),
                _ => Ok(Vec::new()),
            }
        }
        DetailPageKind::Project => {
            match ProjectField::load((target, ProjectFieldType::PageEditorIds))
                .await?
                .field
            {
                ProjectFieldValue::PageEditorIds(ids) => Ok(ids.into_vec()),
                _ => Ok(Vec::new()),
            }
        }
    }
}

/// Persist the manager (page-editor) ids via the peered field for either kind.
async fn store_manager_ids(
    kind: DetailPageKind,
    target: Ulid,
    ids: &[Ulid],
    source: FieldSource,
) -> Result<(), Error> {
    let boxed = ids.to_vec().into_boxed_slice();
    match kind {
        DetailPageKind::Event => {
            EventField::new(target, EventFieldValue::PageEditorIds(boxed))
                .store()
                .await
        }
        DetailPageKind::Project => {
            ProjectField::with_source(target, ProjectFieldValue::PageEditorIds(boxed), source)
                .store()
                .await
        }
    }
}

async fn managers_response(
    target_id: String,
    ids: &[Ulid],
    error: Option<String>,
) -> poem::Result<poem::Response> {
    let resolved: HashMap<Ulid, String> = resolve_user_names(ids.to_vec())
        .await
        .unwrap_or_default()
        .into_iter()
        .collect();

    let managers = ids
        .iter()
        .map(|uid| super::detail_page::ManagerDisplay {
            user_id: uid.to_string(),
            email: resolved
                .get(uid)
                .cloned()
                .unwrap_or_else(|| uid.to_string()),
        })
        .collect();

    let html = ManagersDisplay {
        target_id,
        managers,
        can_edit: true,
        error,
    }
    .render_once()
    .map_err(internal_error)?;
    Ok(Html(html).into_response())
}

/// Resolve an email address to a user id via the user service.
async fn user_id_for_email(email: &str) -> Option<Ulid> {
    let response: UserByEmailResponse = request(
        "user.search.by_email",
        UserByEmailRequest {
            email: email.to_string(),
        },
    )
    .await
    .ok()?;
    if !response.found {
        return None;
    }
    response.user_id.and_then(|s| s.parse::<Ulid>().ok())
}

/// GET /-detail/:id/managers — full managers card.
#[handler]
pub async fn get_managers_display(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(auth_required)?;
    let (kind, target, _) = require_editor(&id, &user).await?;
    let ids = load_manager_ids(kind, target)
        .await
        .map_err(internal_error)?;
    managers_response(id, &ids, None).await
}

/// POST /-detail/:id/managers — grant edit access to the account with this email.
#[handler]
pub async fn add_manager(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
    poem::web::Form(form): poem::web::Form<ManagerForm>,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(auth_required)?;
    let (kind, target, source) = require_editor(&id, &user).await?;

    let mut ids = load_manager_ids(kind, target)
        .await
        .map_err(internal_error)?;

    let email = form.email.trim();
    if email.is_empty() {
        return managers_response(id, &ids, Some("Enter an email address.".to_string())).await;
    }

    let Some(new_id) = user_id_for_email(email).await else {
        return managers_response(
            id,
            &ids,
            Some(format!("No SciStarter account found for {email}.")),
        )
        .await;
    };

    if !ids.contains(&new_id) {
        ids.push(new_id);
        store_manager_ids(kind, target, &ids, source)
            .await
            .map_err(internal_error)?;
        let _ = log_activity(
            None,
            edit_action(kind, source),
            user.user_id,
            target,
            "managers".to_string(),
        )
        .await;
    }

    managers_response(id, &ids, None).await
}

/// GET /-detail/:id/managers/:user_id/edit — inline edit form (change the account).
#[handler]
pub async fn get_manager_edit_form(
    Path((id, user_id)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(auth_required)?;
    require_editor(&id, &user).await?;

    let email = if let Ok(uid) = user_id.parse::<Ulid>() {
        resolve_user_names(vec![uid])
            .await
            .unwrap_or_default()
            .into_iter()
            .next()
            .map(|(_, email)| email)
            .unwrap_or_else(|| user_id.clone())
    } else {
        user_id.clone()
    };

    let html = ManagerEditForm {
        target_id: id,
        user_id,
        email,
    }
    .render_once()
    .map_err(internal_error)?;
    Ok(Html(html).into_response())
}

/// PUT /-detail/:id/managers/:user_id — replace this manager with the account for a new email.
#[handler]
pub async fn update_manager(
    Path((id, user_id)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
    poem::web::Form(form): poem::web::Form<ManagerForm>,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(auth_required)?;
    let (kind, target, source) = require_editor(&id, &user).await?;

    let mut ids = load_manager_ids(kind, target)
        .await
        .map_err(internal_error)?;

    let email = form.email.trim();
    let Some(new_id) = user_id_for_email(email).await else {
        return managers_response(
            id,
            &ids,
            Some(format!("No SciStarter account found for {email}.")),
        )
        .await;
    };

    // Drop the edited entry (and any existing copy of the new account), then
    // add the new account once — leaving exactly one entry for it.
    ids.retain(|uid| uid.to_string() != user_id && *uid != new_id);
    ids.push(new_id);

    store_manager_ids(kind, target, &ids, source)
        .await
        .map_err(internal_error)?;

    let _ = log_activity(
        None,
        edit_action(kind, source),
        user.user_id,
        target,
        "managers".to_string(),
    )
    .await;

    managers_response(id, &ids, None).await
}

/// DELETE /-detail/:id/managers/:user_id — revoke edit access.
#[handler]
pub async fn delete_manager(
    Path((id, user_id)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(auth_required)?;
    let (kind, target, source) = require_editor(&id, &user).await?;

    let ids: Vec<Ulid> = load_manager_ids(kind, target)
        .await
        .map_err(internal_error)?
        .into_iter()
        .filter(|uid| uid.to_string() != user_id)
        .collect();

    store_manager_ids(kind, target, &ids, source)
        .await
        .map_err(internal_error)?;

    let _ = log_activity(
        None,
        edit_action(kind, source),
        user.user_id,
        target,
        "managers".to_string(),
    )
    .await;

    managers_response(id, &ids, None).await
}

// ---------------------------------------------------------------------------
// Participant files (Additional Resources): inline name -> URL list editor,
// gated by the same can_edit permission as the other detail-page fields.
// Rows are identified by their position (index) in the list.
// ---------------------------------------------------------------------------

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_participant_files.stpl.html")]
pub struct ParticipantFilesDisplay {
    pub target_id: String,
    pub files: Vec<ParticipantFile>,
    pub can_edit: bool,
}

#[derive(serde::Deserialize)]
struct ParticipantFileForm {
    name: String,
    url: String,
}

/// Require that the request comes from a user who may edit this project,
/// returning a 401/403 otherwise, and the source their edits are written
/// from.
async fn require_project_editor(
    id: &str,
    user_info: &Option<std::sync::Arc<BasicInfo>>,
) -> poem::Result<FieldSource> {
    let user = user_info.as_ref().ok_or_else(|| {
        poem::Error::from_string(
            "Authentication required",
            poem::http::StatusCode::UNAUTHORIZED,
        )
    })?;

    let (can_edit, is_staff) = check_edit_permission(id, user, DetailPageKind::Project)
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    if !can_edit {
        return Err(poem::Error::from_string(
            "Forbidden",
            poem::http::StatusCode::FORBIDDEN,
        ));
    }

    Ok(edit_source(is_staff))
}

async fn load_participant_files(id: &str) -> poem::Result<Vec<ParticipantFile>> {
    let ulid = Ulid::from_string(id).map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::BAD_REQUEST)
    })?;

    let field = ProjectField::load((ulid, ProjectFieldType::ParticipantFiles))
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    Ok(match field.field {
        ProjectFieldValue::ParticipantFiles(files) => files.into_vec(),
        _ => Vec::new(),
    })
}

/// Persist the list and return the freshly rendered editor fragment. Called
/// only from edit handlers, so `can_edit` is necessarily true.
async fn store_and_render_participant_files(
    id: &str,
    files: Vec<ParticipantFile>,
    source: FieldSource,
) -> poem::Result<poem::Response> {
    let ulid = Ulid::from_string(id).map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::BAD_REQUEST)
    })?;

    ProjectField::with_source(
        ulid,
        ProjectFieldValue::ParticipantFiles(files.clone().into_boxed_slice()),
        source,
    )
    .store()
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let html = ParticipantFilesDisplay {
        target_id: id.to_string(),
        files,
        can_edit: true,
    }
    .render_once()
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

fn log_project_edit(
    user_info: &Option<std::sync::Arc<BasicInfo>>,
    id: &str,
    source: FieldSource,
    detail: String,
) {
    if let (Some(user), Ok(ulid)) = (user_info.as_ref(), Ulid::from_string(id)) {
        let user_id = user.user_id;
        let action = edit_action(DetailPageKind::Project, source);
        tokio::spawn(async move {
            let _ = log_activity(None, action, user_id, ulid, detail).await;
        });
    }
}

/// GET /-detail/:id/participant-files — full Additional Resources fragment.
#[handler]
pub async fn get_participant_files_display(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let can_edit = if let Some(ref user) = user_info {
        check_edit_permission(&id, user, DetailPageKind::Project)
            .await
            .map(|(c, _)| c)
            .unwrap_or(false)
    } else {
        false
    };

    let files = load_participant_files(&id).await?;

    let html = ParticipantFilesDisplay {
        target_id: id,
        files,
        can_edit,
    }
    .render_once()
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

/// POST /-detail/:id/participant-files — add a resource.
#[handler]
pub async fn add_participant_file(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
    poem::web::Form(form): poem::web::Form<ParticipantFileForm>,
) -> poem::Result<poem::Response> {
    let source = require_project_editor(&id, &user_info).await?;

    let mut files = load_participant_files(&id).await?;
    let new_file = ParticipantFile {
        name: form.name.trim().to_string(),
        url: form.url.trim().to_string(),
    };
    if !new_file.url.is_empty() {
        files.push(new_file);
    }

    log_project_edit(&user_info, &id, source, "add=participant_files".to_string());
    store_and_render_participant_files(&id, files, source).await
}

/// PUT /-detail/:id/participant-files/:index — edit a resource in place.
#[handler]
pub async fn update_participant_file(
    Path((id, index)): Path<(String, usize)>,
    Authenticated(user_info): Authenticated,
    poem::web::Form(form): poem::web::Form<ParticipantFileForm>,
) -> poem::Result<poem::Response> {
    let source = require_project_editor(&id, &user_info).await?;

    let mut files = load_participant_files(&id).await?;
    if index >= files.len() {
        return Err(poem::Error::from_string(
            "Resource not found",
            poem::http::StatusCode::NOT_FOUND,
        ));
    }
    files[index] = ParticipantFile {
        name: form.name.trim().to_string(),
        url: form.url.trim().to_string(),
    };

    log_project_edit(
        &user_info,
        &id,
        source,
        format!("participant_files[{}]", index),
    );
    store_and_render_participant_files(&id, files, source).await
}

/// DELETE /-detail/:id/participant-files/:index — remove a resource.
#[handler]
pub async fn delete_participant_file(
    Path((id, index)): Path<(String, usize)>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let source = require_project_editor(&id, &user_info).await?;

    let mut files = load_participant_files(&id).await?;
    if index < files.len() {
        files.remove(index);
    }

    log_project_edit(
        &user_info,
        &id,
        source,
        format!("remove=participant_files[{}]", index),
    );
    store_and_render_participant_files(&id, files, source).await
}

// ---------------------------------------------------------------------------
// Additional fields (bottom accordion for editors)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AdditionalField {
    pub field_type: String,
    pub label: String,
    pub current_value: String,
}

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_additional_fields.stpl.html")]
pub struct AdditionalFieldsDisplay {
    pub target_id: String,
    pub fields: Vec<AdditionalField>,
}

/// Project additional field types (not displayed on the main page)
static PROJECT_ADDITIONAL_FIELDS: &[&str] = &[
    "status",
    "difficulty",
    "search_terms",
    "signup_url",
    "opengraph_description",
    "begin",
    "end",
    "project_timing",
    "year_started",
    "participation_degree",
    "on_demand",
    "in_person",
    "indoors",
    "outdoors",
    "materials",
    "gear",
    "outcomes_label",
    "outcomes_link",
    "time_required",
    "consent_statement",
    "data_publication_url",
    "data_license",
    "university_affiliation",
];

/// Event additional field types
static EVENT_ADDITIONAL_FIELDS: &[&str] = &["status"];

/// Load additional fields for the bottom accordion
pub async fn load_additional_fields(
    target_id: &str,
    kind: DetailPageKind,
) -> Result<Vec<AdditionalField>, Error> {
    let field_types: &[&str] = match kind {
        DetailPageKind::Project => PROJECT_ADDITIONAL_FIELDS,
        DetailPageKind::Event => EVENT_ADDITIONAL_FIELDS,
    };

    let mut fields = Vec::new();
    for &ft in field_types {
        if let Some(meta) = field_meta(ft, kind) {
            let value = read_field_value(target_id, ft, kind)
                .await
                .unwrap_or_default();
            fields.push(AdditionalField {
                field_type: ft.to_string(),
                label: meta.label.to_string(),
                current_value: value,
            });
        }
    }
    Ok(fields)
}
