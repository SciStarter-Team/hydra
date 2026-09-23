use datafusion::arrow::datatypes::{DataType, Field, Schema};
use serde::{Deserialize, Serialize};

use super::table::{TableRegistration, TableRow};

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
    pub device_id: String,
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
            Field::new("device_id", DataType::Utf8, false),
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
    pub device_id: String,
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
            Field::new("device_id", DataType::Utf8, false),
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
    pub user_id: Option<String>,
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
            Field::new("user_id", DataType::Utf8, true),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailEventRow {
    pub when: String,
    pub event_type: String,
    pub sending_id: String,
    pub email_address: String,
    pub user_id: Option<String>,
    pub reason: Option<String>,
}

impl TableRow for EmailEventRow {
    const TABLE_NAME: &'static str = "email_events";

    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("when", DataType::Utf8, false),
            Field::new("event_type", DataType::Utf8, false),
            Field::new("sending_id", DataType::Utf8, false),
            Field::new("email_address", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, true),
            Field::new("reason", DataType::Utf8, true),
        ])
    }
}

inventory::submit! {
    TableRegistration {
        name: EmailEventRow::TABLE_NAME,
        schema: EmailEventRow::schema,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeEventRow {
    pub when: String,
    pub event_id: String,
    pub exchange_id: String,
    pub device_id: String,
    pub interaction_id: String,
    pub user_id: Option<String>,
    pub page: String,
    pub page_id: Option<String>,
    pub action: String,
    pub extra: Option<String>,
}

impl TableRow for ExchangeEventRow {
    const TABLE_NAME: &'static str = "exchange_events";

    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("when", DataType::Utf8, false),
            Field::new("event_id", DataType::Utf8, false),
            Field::new("exchange_id", DataType::Utf8, false),
            Field::new("device_id", DataType::Utf8, false),
            Field::new("interaction_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, true),
            Field::new("page", DataType::Utf8, false),
            Field::new("page_id", DataType::Utf8, true),
            Field::new("action", DataType::Utf8, false),
            Field::new("extra", DataType::Utf8, true),
        ])
    }
}

inventory::submit! {
    TableRegistration {
        name: ExchangeEventRow::TABLE_NAME,
        schema: ExchangeEventRow::schema,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchEventRow {
    pub when: String,
    pub device_id: String,
    pub interaction_id: String,
    pub user_id: Option<String>,
    pub query_text: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub location_name: Option<String>,
    pub location_type: Option<String>,
    pub topics: Option<String>,
    pub audiences: Option<String>,
    pub activities: Option<String>,
    pub misc_filters: Option<String>,
    pub show_completed: bool,
    pub kinds: Option<String>,
    pub show_past: Option<bool>,
    pub event_start: Option<String>,
    pub event_end: Option<String>,
    pub page: i32,
    pub result_count: i32,
    pub search_type: String,
}

impl TableRow for SearchEventRow {
    const TABLE_NAME: &'static str = "search_events";

    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("when", DataType::Utf8, false),
            Field::new("device_id", DataType::Utf8, false),
            Field::new("interaction_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, true),
            Field::new("query_text", DataType::Utf8, true),
            Field::new("lat", DataType::Float64, true),
            Field::new("lng", DataType::Float64, true),
            Field::new("location_name", DataType::Utf8, true),
            Field::new("location_type", DataType::Utf8, true),
            Field::new("topics", DataType::Utf8, true),
            Field::new("audiences", DataType::Utf8, true),
            Field::new("activities", DataType::Utf8, true),
            Field::new("misc_filters", DataType::Utf8, true),
            Field::new("show_completed", DataType::Boolean, false),
            Field::new("kinds", DataType::Utf8, true),
            Field::new("show_past", DataType::Boolean, true),
            Field::new("event_start", DataType::Utf8, true),
            Field::new("event_end", DataType::Utf8, true),
            Field::new("page", DataType::Int32, false),
            Field::new("result_count", DataType::Int32, false),
            Field::new("search_type", DataType::Utf8, false),
        ])
    }
}

inventory::submit! {
    TableRegistration {
        name: SearchEventRow::TABLE_NAME,
        schema: SearchEventRow::schema,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareEventRow {
    pub when: String,
    pub target_id: String,
    pub platform: String,
    pub user_id: Option<String>,
    pub device_id: String,
    pub interaction_id: String,
}

impl TableRow for ShareEventRow {
    const TABLE_NAME: &'static str = "share_events";

    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("when", DataType::Utf8, false),
            Field::new("target_id", DataType::Utf8, false),
            Field::new("platform", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, true),
            Field::new("device_id", DataType::Utf8, false),
            Field::new("interaction_id", DataType::Utf8, false),
        ])
    }
}

inventory::submit! {
    TableRegistration {
        name: ShareEventRow::TABLE_NAME,
        schema: ShareEventRow::schema,
    }
}
