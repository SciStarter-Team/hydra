use arrow::datatypes::{DataType, Field, Schema};
use serde::{Deserialize, Serialize};

use super::table::{TableRow, TableRegistration};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityLogRow {
    pub when: String,
    pub action: String,
    pub user_id: Option<String>,
    pub target: Option<String>,
    pub extra: Option<String>,
}

impl TableRow for ActivityLogRow {
    const TABLE_NAME: &'static str = "activity_log";

    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("when", DataType::Utf8, false),
            Field::new("action", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, true),
            Field::new("target", DataType::Utf8, true),
            Field::new("extra", DataType::Utf8, true),
        ])
    }
}

inventory::submit! {
    TableRegistration {
        name: ActivityLogRow::TABLE_NAME,
        schema: ActivityLogRow::schema,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageViewRow {
    pub when: String,
    pub ip_address: String,
    pub session_id: String,
    pub interaction_id: String,
    pub user_id: Option<String>,
    pub path: String,
    pub referrer: Option<String>,
    pub query: Option<String>,
    pub geolocation: String,
}

impl TableRow for PageViewRow {
    const TABLE_NAME: &'static str = "page_views";

    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("when", DataType::Utf8, false),
            Field::new("ip_address", DataType::Utf8, false),
            Field::new("session_id", DataType::Utf8, false),
            Field::new("interaction_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, true),
            Field::new("path", DataType::Utf8, false),
            Field::new("referrer", DataType::Utf8, true),
            Field::new("query", DataType::Utf8, true),
            Field::new("geolocation", DataType::Utf8, false),
        ])
    }
}

inventory::submit! {
    TableRegistration {
        name: PageViewRow::TABLE_NAME,
        schema: PageViewRow::schema,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickthroughRow {
    pub when: String,
    pub ip_address: String,
    pub session_id: String,
    pub interaction_id: String,
    pub user_id: Option<String>,
    pub from_path: String,
    pub to_url: String,
    pub link_type: String,
    pub program_id: Option<String>,
    pub geolocation: String,
}

impl TableRow for ClickthroughRow {
    const TABLE_NAME: &'static str = "clickthroughs";

    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("when", DataType::Utf8, false),
            Field::new("ip_address", DataType::Utf8, false),
            Field::new("session_id", DataType::Utf8, false),
            Field::new("interaction_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, true),
            Field::new("from_path", DataType::Utf8, false),
            Field::new("to_url", DataType::Utf8, false),
            Field::new("link_type", DataType::Utf8, false),
            Field::new("program_id", DataType::Utf8, true),
            Field::new("geolocation", DataType::Utf8, false),
        ])
    }
}

inventory::submit! {
    TableRegistration {
        name: ClickthroughRow::TABLE_NAME,
        schema: ClickthroughRow::schema,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRow {
    pub when: String,
    pub entity_id: String,
    pub user_id: String,
    pub active: bool,
    pub consented: Option<String>,
    pub contacted: Option<String>,
    pub explicit: bool,
    pub notified: bool,
    pub activity: String,
    pub reason: String,
}

impl TableRow for JoinRow {
    const TABLE_NAME: &'static str = "join_table";

    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("when", DataType::Utf8, false),
            Field::new("entity_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, false),
            Field::new("active", DataType::Boolean, false),
            Field::new("consented", DataType::Utf8, true),
            Field::new("contacted", DataType::Utf8, true),
            Field::new("explicit", DataType::Boolean, false),
            Field::new("notified", DataType::Boolean, false),
            Field::new("activity", DataType::Utf8, false),
            Field::new("reason", DataType::Utf8, false),
        ])
    }
}

inventory::submit! {
    TableRegistration {
        name: JoinRow::TABLE_NAME,
        schema: JoinRow::schema,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipationRow {
    pub origin: i16,
    pub when: String,
    pub duration: f64,
    pub where_point: Option<String>,
    pub magnitude: i32,
    pub authenticated: bool,
    pub extra: String,
    pub fidelity: f64,
    pub entity_id: String,
    pub user_id: String,
    pub task_name: Option<String>,
}

impl TableRow for ParticipationRow {
    const TABLE_NAME: &'static str = "participation";

    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("origin", DataType::Int16, false),
            Field::new("when", DataType::Utf8, false),
            Field::new("duration", DataType::Float64, false),
            Field::new("where_point", DataType::Utf8, true),
            Field::new("magnitude", DataType::Int32, false),
            Field::new("authenticated", DataType::Boolean, false),
            Field::new("extra", DataType::Utf8, false),
            Field::new("fidelity", DataType::Float64, false),
            Field::new("entity_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, false),
            Field::new("task_name", DataType::Utf8, true),
        ])
    }
}

inventory::submit! {
    TableRegistration {
        name: ParticipationRow::TABLE_NAME,
        schema: ParticipationRow::schema,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkRow {
    pub when: String,
    pub entity_id: String,
    pub user_id: String,
    pub active: bool,
    pub activity: String,
    pub contacted: Option<String>,
}

impl TableRow for BookmarkRow {
    const TABLE_NAME: &'static str = "bookmark";

    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("when", DataType::Utf8, false),
            Field::new("entity_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, false),
            Field::new("active", DataType::Boolean, false),
            Field::new("activity", DataType::Utf8, false),
            Field::new("contacted", DataType::Utf8, true),
        ])
    }
}

inventory::submit! {
    TableRegistration {
        name: BookmarkRow::TABLE_NAME,
        schema: BookmarkRow::schema,
    }
}
