use anyhow::Error;
use common::{message::user::BasicInfo, service::log_activity, shared::db, timestring::TimeString};
use poem::{
    IntoResponse, handler,
    web::{Html, Path},
};
use sailfish::TemplateOnce;
use ulid::Ulid;

use crate::event::field::{EventField, EventFieldValue};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::project::field::{ProjectField, ProjectFieldValue};
use crate::project::field_source::FieldSource;

use super::detail_page::DetailPageKind;

// ---------------------------------------------------------------------------
// Field metadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum FieldInputKind {
    Text,
    Textarea,
    RichText,
    Url,
    Boolean,
    Select(&'static [(&'static str, &'static str)]),
    MultiCheckbox(&'static [(&'static str, &'static str)]),
}

#[derive(Debug, Clone)]
pub struct FieldMeta {
    pub label: &'static str,
    pub input_kind: FieldInputKind,
}

static AUDIENCE_OPTIONS: &[(&str, &str)] = &[
    ("Families", "Families"),
    ("Adults", "Adults"),
    ("Targeted group", "Targeted group"),
    ("Teens (13-17)", "Teens (13-17)"),
    ("Elementary school (6-10)", "Elementary school (6-10)"),
    ("Middle school (11-13)", "Middle school (11-13)"),
];

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
            label: "Image URL",
            input_kind: FieldInputKind::Url,
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
            input_kind: FieldInputKind::MultiCheckbox(AUDIENCE_OPTIONS),
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
            input_kind: FieldInputKind::Text,
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
            label: "Regions Description",
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
            input_kind: FieldInputKind::Select(&[
                ("Conference", "Conference"),
                ("Festival", "Festival"),
                ("Meeting", "Meeting"),
                ("Training", "Training"),
                ("Workshop", "Workshop"),
                ("Webinar", "Webinar"),
                ("Other", "Other"),
            ]),
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

/// Check whether the user can edit the given project/event.
/// Returns (can_edit, is_staff_editor).
pub async fn check_edit_permission(
    target_id: &str,
    user: &BasicInfo,
    kind: DetailPageKind,
) -> Result<(bool, bool), Error> {
    if user.is_staff || user.is_superuser {
        return Ok((true, true));
    }

    let user_str = user.user_id.to_string();
    let exists = match kind {
        DetailPageKind::Project => {
            sqlx::query_scalar!(
                r#"SELECT 1 as "v!: i32" FROM project_page_editors WHERE project_id = ? AND user_id = ?"#,
                target_id,
                user_str
            )
            .fetch_optional(db())
            .await?
            .is_some()
        }
        DetailPageKind::Event => {
            sqlx::query_scalar!(
                r#"SELECT 1 as "v!: i32" FROM event_page_editors WHERE event_id = ? AND user_id = ?"#,
                target_id,
                user_str
            )
            .fetch_optional(db())
            .await?
            .is_some()
        }
    };

    Ok((exists, false))
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
        "event_type" => EventFieldValue::EventType(value.to_string()),
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

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_field_edit.stpl.html")]
struct FieldEditForm {
    target_id: String,
    field_type: String,
    label: String,
    input_kind: FieldInputKind,
    current_value: String,
}

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_field_display.stpl.html")]
pub struct FieldDisplay {
    pub target_id: String,
    pub field_type: String,
    pub label: String,
    pub can_edit: bool,
    pub content_html: String,
}

// ---------------------------------------------------------------------------
// Form data
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct FieldForm {
    #[serde(deserialize_with = "crate::form::deserialize_string_or_vec")]
    value: Vec<String>,
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

    let html = FieldEditForm {
        target_id: id,
        field_type,
        label: meta.label.to_string(),
        input_kind: meta.input_kind,
        current_value,
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

    let source = if is_staff {
        FieldSource::Staff
    } else {
        FieldSource::Form
    };

    match kind {
        DetailPageKind::Project => {
            save_project_field(target_ulid, &field_type, &form.value, source).await
        }
        DetailPageKind::Event => save_event_field(target_ulid, &field_type, &form.value).await,
    }
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    // Log activity
    let action = match kind {
        DetailPageKind::Project => "project-edit",
        DetailPageKind::Event => "event-edit",
    };
    let _ = log_activity(None, action, user.user_id, target_ulid, field_type.clone()).await;

    // Return updated display
    let current_value = read_field_value(&id, &field_type, kind)
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    let display_html = format_display_value(&field_type, &current_value, kind);

    let html = FieldDisplay {
        target_id: id,
        field_type,
        label: meta.label.to_string(),
        can_edit: true,
        content_html: display_html,
    }
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

    let display_html = format_display_value(&field_type, &current_value, kind);

    let html = FieldDisplay {
        target_id: id,
        field_type,
        label: meta.label.to_string(),
        can_edit,
        content_html: display_html,
    }
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
    r#"<span class="text-gray-400 italic">(not set)</span>"#
}

/// Format a field value for display as HTML
fn format_display_value(field_type: &str, value: &str, kind: DetailPageKind) -> String {
    let empty = value.is_empty();

    match field_type {
        // Ribbon-box fields (top-level styled blocks)
        "goal" if kind == DetailPageKind::Project => {
            if empty {
                ribbon_box("Goal", not_set())
            } else {
                ribbon_box("Goal", html_escape(value))
            }
        }
        "task" if kind == DetailPageKind::Project => {
            if empty {
                ribbon_box("Task", not_set())
            } else {
                ribbon_box("Task", html_escape(value))
            }
        }
        "description" => ribbon_box("Description", if empty { not_set() } else { value }),
        "how_to_join" => ribbon_box("How to Get Started", if empty { not_set() } else { value }),

        // Location fields — simple inline display
        "location_name" => {
            if empty {
                not_set().to_string()
            } else {
                format!(
                    r#"<p class="mb-1 font-semibold">{}</p>"#,
                    html_escape(value)
                )
            }
        }
        "location_description" | "address" => {
            if empty {
                not_set().to_string()
            } else {
                format!(r#"<p class="mb-1">{}</p>"#, html_escape(value))
            }
        }

        // More Info accordion interior fields
        "video_url" => {
            if empty {
                ribbon_subbox("Video", not_set())
            } else {
                ribbon_subbox(
                    "Video",
                    &format!(
                        r#"<a href="{}" class="text-primary-action hover:underline" target="_blank" rel="noopener">Check out the how-to video</a>"#,
                        html_escape(value)
                    ),
                )
            }
        }
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
                html_escape(value)
            };
            ribbon_subbox("Total Expense", &content)
        }
        "time_per_participation" => {
            if empty {
                ribbon_subbox("Spend the Time", not_set())
            } else {
                ribbon_subbox("Spend the Time", html_escape(value))
            }
        }
        "participation_schedule" => {
            if empty {
                ribbon_subbox("Ideal Frequency", not_set())
            } else {
                ribbon_subbox("Ideal Frequency", html_escape(value))
            }
        }
        "training_materials" => {
            if empty {
                ribbon_subbox("Training Materials", not_set())
            } else {
                ribbon_subbox(
                    "Training Materials",
                    &format!(
                        r#"<a href="{0}" class="text-primary-action hover:underline" target="_blank" rel="noopener">{0}</a>"#,
                        html_escape(value)
                    ),
                )
            }
        }
        "classroom_materials" => {
            if empty {
                ribbon_subbox("Classroom Materials", not_set())
            } else {
                ribbon_subbox(
                    "Classroom Materials",
                    &format!(
                        r#"<a href="{0}" class="text-primary-action hover:underline" target="_blank" rel="noopener">{0}</a>"#,
                        html_escape(value)
                    ),
                )
            }
        }

        // Boolean fields
        "entirely_online" | "on_demand" | "in_person" | "indoors" | "outdoors" => {
            if value == "true" {
                "Yes".to_string()
            } else {
                "No".to_string()
            }
        }

        // Default: escaped text
        _ => {
            if empty {
                not_set().to_string()
            } else {
                html_escape(value)
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

    let (can_edit, _) = check_edit_permission(&id, &user, DetailPageKind::Event)
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
        "event-edit",
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

    let (can_edit, _) = check_edit_permission(&id, &user, DetailPageKind::Event)
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
        "event-edit",
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

    let (can_edit, _) = check_edit_permission(&id, &user, DetailPageKind::Event)
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
        "event-edit",
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
            Some(super::detail_page::EventInstanceDisplay {
                begin: begin.format_display(),
                end: end.format_display(),
            })
        })
        .collect())
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
    "regions_description",
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
