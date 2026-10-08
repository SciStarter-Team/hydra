use crate::{
    component::{Component, Head, NumberedPagination, StandardLayout},
    form::QsQuery,
    local_time::{TimeFormat, local_time},
    middleware::{
        analytics::{Device, Interaction},
        authorization::Authenticated,
    },
    project::ProjectStatus,
};
use anyhow::Error;
use common::shared::db;
use jiff::civil::Date;
use poem::{Endpoint, Response, Result, Route, get, handler, session::Session, web::Query};
use sailfish::TemplateOnce;
use serde::Deserialize;

/// A search result for public entity search
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub slug: String,
    pub entity_type: String,
    pub title: String,
}

impl SearchResult {
    /// Canonical link for this result based on its entity type.
    pub fn link(&self) -> String {
        format!("{}{}", path_prefix_for(&self.entity_type), self.slug)
    }
}

/// Map an entity-type label to its URL path prefix. Accepts both the
/// stored kind name (`Project`/`Event`) and the autocomplete display
/// label, so it works for every search result type.
fn path_prefix_for(entity_type: &str) -> &'static str {
    match entity_type {
        "Project" => "/projects/",
        "Event" => "/events/",
        _ => "/",
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
}

fn deserialize_optional_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => s.parse().map(Some).map_err(serde::de::Error::custom),
    }
}

/// The `kind[]` values the finder's entity-kind toggles submit.
pub(crate) const PROJECT_KIND: &str = "project";
pub(crate) const EVENT_KIND: &str = "event";

/// Submitted by a hidden field, so that a form with both toggles cleared says
/// so rather than looking like a form that was never filled in.
const NO_KIND: &str = "none";

#[derive(Debug, Clone, Deserialize)]
pub struct FinderQuery {
    pub q: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub lat: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub lng: Option<f64>,
    pub location_name: Option<String>,
    pub page: Option<u32>,
    pub location_type: Option<String>,
    #[serde(default)]
    pub topic: Vec<String>,
    #[serde(default)]
    pub audience: Vec<String>,
    #[serde(default)]
    pub activity: Vec<String>,
    pub show_completed: Option<String>,
    #[serde(default)]
    pub misc_filter: Vec<String>,

    /// Which kinds of entity to search, as the sidebar's toggles submit them:
    /// [`PROJECT_KIND`], [`EVENT_KIND`], and the hidden [`NO_KIND`] that lets a
    /// form say "neither" -- unchecked boxes submit nothing, so without it an
    /// empty list would be indistinguishable from an unfiltered `/finder`.
    #[serde(default)]
    pub kind: Vec<String>,

    /// The earliest and latest dates, `YYYY-MM-DD`, an event may run on.
    pub start: Option<String>,
    pub end: Option<String>,

    /// Include events that are over, the way `show_completed` includes
    /// projects that are.
    pub show_past: Option<String>,
}

/// What the finder's sidebar does with a filter only one kind of result
/// understands -- an age group, which no event records, or a date range, which
/// no project has instances for -- when the search covers both kinds.
///
/// The two answers are a policy choice, not a mechanism one, so the choice
/// lives here in one place: [`SingleKindFilters::CURRENT`] is what the site
/// serves, [`FinderQuery::as_offered`] applies it to the search, and the
/// sidebar's form carries [`SingleKindFilters::form_class`] so that
/// `static/css/pages/finder.css` shows exactly the fields that are in force.
/// Changing the one constant moves all three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SingleKindFilters {
    /// Held back until its own kind is the whole search. Results then answer
    /// every filter on display, at the price of a plainer sidebar whenever
    /// both kinds are searched.
    HeldBack,

    /// Offered whenever its kind is in the search, marked with the kind it
    /// narrows. The full sidebar is always to hand, at the price of results
    /// where one kind has been narrowed and the other has not.
    ///
    /// Allowed dead: this is the arm of a compile-time switch, so whichever
    /// one [`SingleKindFilters::CURRENT`] does not name is never constructed
    /// outside the tests. Drop the attribute if `CURRENT` comes to name this
    /// one -- the other will need it instead.
    #[allow(dead_code)]
    Marked,
}

impl SingleKindFilters {
    /// What the finder does today.
    pub(crate) const CURRENT: SingleKindFilters = SingleKindFilters::HeldBack;

    /// The class the sidebar's form carries under this policy, which is how
    /// the stylesheet follows it.
    pub(crate) fn form_class(self) -> &'static str {
        match self {
            SingleKindFilters::HeldBack => "",
            SingleKindFilters::Marked => "marks-single-kind",
        }
    }
}

impl FinderQuery {
    /// The query as the finder's sidebar offers it for a search of `kinds`.
    ///
    /// Whatever the sidebar does not offer must not narrow anything either, or
    /// the results would be answering a question the page has stopped asking.
    /// Dropping it here is what makes the two agree, under either
    /// [`SingleKindFilters`] policy. The value itself stays in the query
    /// string, so a filter set aside comes back into force with the search
    /// that offers it.
    ///
    /// This is the sidebar's rule, applied where the sidebar's query arrives.
    /// The concierge asks for what it means and gets it.
    pub(crate) fn as_offered(
        &self,
        kinds: SearchKinds,
        single_kind: SingleKindFilters,
    ) -> FinderQuery {
        let (projects, events) = match single_kind {
            SingleKindFilters::HeldBack => (
                kinds == SearchKinds::ProjectsOnly,
                kinds == SearchKinds::EventsOnly,
            ),
            SingleKindFilters::Marked => (kinds.projects(), kinds.events()),
        };

        let mut offered = self.clone();

        if !projects {
            offered.audience.clear();
            offered.activity.clear();
            offered.misc_filter.clear();
            offered.show_completed = None;
        }

        if !events {
            offered.start = None;
            offered.end = None;
            offered.show_past = None;
        }

        offered
    }

    /// The finder's own query string for this search, without `page`:
    /// everything a link needs to run it again. The pagination links and the
    /// concierge's links into the finder both go through here, so a filter
    /// can only be left out of a link in one place.
    pub(crate) fn to_query_string(&self) -> String {
        let mut params = Vec::new();

        if let Some(q) = self.q.as_deref().filter(|q| !q.is_empty()) {
            params.push(format!("q={}", urlencoding::encode(q)));
        }

        if let (Some(lat), Some(lng)) = (self.lat, self.lng) {
            params.push(format!("lat={lat}"));
            params.push(format!("lng={lng}"));
        }

        if let Some(name) = self
            .location_name
            .as_deref()
            .filter(|name| !name.is_empty())
        {
            params.push(format!("location_name={}", urlencoding::encode(name)));
        }

        if let Some(location_type) = self
            .location_type
            .as_deref()
            .filter(|location_type| *location_type != "either")
        {
            params.push(format!("location_type={location_type}"));
        }

        for (name, values) in [
            ("topic", &self.topic),
            ("audience", &self.audience),
            ("activity", &self.activity),
            ("misc_filter", &self.misc_filter),
        ] {
            for value in values {
                params.push(format!("{name}[]={}", urlencoding::encode(value)));
            }
        }

        if self.show_completed.is_some() {
            params.push("show_completed=on".to_string());
        }

        if self.show_past.is_some() {
            params.push("show_past=on".to_string());
        }

        if let Some(start) = parse_date(&self.start) {
            params.push(format!("start={start}"));
        }

        if let Some(end) = parse_date(&self.end) {
            params.push(format!("end={end}"));
        }

        // Both kinds is what a link with no kinds at all already means.
        let kinds = SearchKinds::from_query(self);
        if !matches!(kinds, SearchKinds::Both) {
            for kind in kinds.as_params() {
                params.push(format!("kind[]={kind}"));
            }
        }

        params.join("&")
    }
}

#[derive(Debug, TemplateOnce)]
#[template(path = "search/autocomplete_results.stpl.html")]
pub struct SearchAutocompleteResults {
    pub results: Vec<SearchResult>,
}

/// Build a safe FTS5 prefix-match query from raw user input.
///
/// The result is always passed as a bind parameter to `MATCH`, so it cannot
/// inject SQL. It is, however, interpreted as an FTS5 *query expression*, in
/// which `"`, `*`, `:`, `(`, and words like `OR`/`NEAR` are operators. Passing
/// raw user text lets a user inject those operators (causing query errors / mild
/// DoS or altering matching). Wrapping the text in a double-quoted string
/// (escaping embedded quotes) and appending `*` treats it as a literal prefix
/// search — a quoted string followed by `*` is a valid FTS5 prefix token.
pub fn fts5_prefix_query(input: &str) -> String {
    let escaped = input.trim().replace('"', "\"\"");
    format!("\"{}\"*", escaped)
}

/// Build a safe FTS5 query matching every word of the input as its own
/// prefix token (space-separated terms are an implicit AND in FTS5). Looser
/// than [`fts5_prefix_query`]'s exact-phrase match: "bird migration" matches
/// any document containing both a "bird"-prefixed and a "migration"-prefixed
/// word, in any order. Each word gets the same quote-escaping treatment, so
/// the same injection-safety argument applies.
pub fn fts5_all_words_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// How free-text input is turned into an FTS5 query expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FtsTermStyle {
    /// The whole input as one exact phrase prefix — the `/finder` behavior.
    Phrase,
    /// Every word as an independent prefix term, AND-ed. Used by the
    /// concierge, where the model supplies keyword lists rather than
    /// phrases.
    AllWords,
}

impl FtsTermStyle {
    fn term_for(self, input: &str) -> String {
        match self {
            FtsTermStyle::Phrase => fts5_prefix_query(input),
            FtsTermStyle::AllWords => fts5_all_words_query(input),
        }
    }
}

/// Which entity kinds a finder search covers. The finder's sidebar has a
/// toggle per kind, and the concierge narrows to one kind when the visitor
/// asks specifically for projects or for events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchKinds {
    Both,
    ProjectsOnly,
    EventsOnly,
    Neither,
}

impl SearchKinds {
    /// The kinds a finder query asks for. No `kind[]` at all -- a bare
    /// `/finder`, or a link written before the toggles existed -- means both.
    pub(crate) fn from_query(query: &FinderQuery) -> Self {
        if query.kind.is_empty() {
            return SearchKinds::Both;
        }

        let projects = query.kind.iter().any(|kind| kind == PROJECT_KIND);
        let events = query.kind.iter().any(|kind| kind == EVENT_KIND);

        match (projects, events) {
            (true, true) => SearchKinds::Both,
            (true, false) => SearchKinds::ProjectsOnly,
            (false, true) => SearchKinds::EventsOnly,
            (false, false) => SearchKinds::Neither,
        }
    }

    /// The `kind[]` values that ask for exactly these kinds, which is how a
    /// link or a log line records them.
    pub(crate) fn as_params(self) -> &'static [&'static str] {
        match self {
            SearchKinds::Both => &[PROJECT_KIND, EVENT_KIND],
            SearchKinds::ProjectsOnly => &[PROJECT_KIND],
            SearchKinds::EventsOnly => &[EVENT_KIND],
            SearchKinds::Neither => &[NO_KIND],
        }
    }

    fn projects(self) -> bool {
        matches!(self, SearchKinds::Both | SearchKinds::ProjectsOnly)
    }

    fn events(self) -> bool {
        matches!(self, SearchKinds::Both | SearchKinds::EventsOnly)
    }
}

/// Search published entities using FTS
pub async fn search_entities(query: &str) -> Result<Vec<SearchResult>, Error> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    // Use FTS5 MATCH with prefix search
    let search_term = fts5_prefix_query(query);

    // This selects columns directly from an FTS5 virtual table. The compile-time
    // `query!` macro cannot describe FTS5 virtual-table columns under sqlx 0.9
    // (the describe path recurses without terminating), so use the runtime
    // `query_as` API with an explicit row type instead.
    #[derive(sqlx::FromRow)]
    struct EntityFtsRow {
        slug: Option<String>,
        entity_type: Option<String>,
        title: Option<String>,
    }

    let rows = sqlx::query_as::<_, EntityFtsRow>(
        r#"SELECT f.slug, f.entity_type, f.title
           FROM entities_fts f
           JOIN entities e ON f.slug = e.slug
           WHERE entities_fts MATCH ?
             AND e.publish = 1
           GROUP BY f.slug
           ORDER BY rank
           LIMIT 20"#,
    )
    .bind(search_term)
    .fetch_all(db())
    .await?;

    let results = rows
        .into_iter()
        .filter_map(|row| {
            let entity_type = row.entity_type?;
            let display_type = match entity_type.to_lowercase().as_str() {
                "project" => "Project",
                "event" => "Event",
                _ => "Page",
            };
            Some(SearchResult {
                slug: row.slug?,
                entity_type: display_type.to_string(),
                title: row.title?,
            })
        })
        .collect();

    Ok(results)
}

pub fn routes() -> impl Endpoint {
    Route::new().at("/autocomplete", get(handle_search_autocomplete))
}

#[handler]
async fn handle_search_autocomplete(Query(query): Query<SearchQuery>) -> Result<Response> {
    let search_term = query.q.as_deref().unwrap_or("");

    let results = search_entities(search_term).await.map_err(|e| {
        tracing::error!("Search error: {}", e);
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let template = SearchAutocompleteResults { results };

    let html = template.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder().content_type("text/html").body(html))
}

// --- Finder page ---

#[derive(Debug, Clone)]
pub struct FinderResult {
    pub slug: String,
    pub entity_type: String,
    pub title: String,
    pub location_display: String,
    pub image: String,
    pub description_snippet: String,
    pub distance_km: Option<f64>,
    pub goal: String,
    pub task: String,
    pub event_begin: Option<String>,
    pub is_completed: bool,
    pub geo_json: Option<String>,
}

impl FinderResult {
    /// Canonical link for this result based on its entity type.
    pub fn link(&self) -> String {
        format!("{}{}", path_prefix_for(&self.entity_type), self.slug)
    }

    pub fn distance_mi(&self) -> Option<f64> {
        self.distance_km.map(|km| km * 0.621371)
    }

    pub fn event_date_html(&self, format: TimeFormat) -> String {
        local_time(self.event_begin.as_deref().unwrap_or(""), format)
    }
}

#[derive(TemplateOnce)]
#[template(path = "pages/finder.stpl.html")]
struct FinderPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<FinderContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "pages/finder_content.stpl.html")]
struct FinderContent {
    results: Vec<FinderResult>,
    query: String,
    location_name: String,
    lat: String,
    lng: String,
    geosearch_key: String,
    pagination: Option<NumberedPagination>,
    // Filter options
    available_topics: Vec<String>,
    available_audiences: Vec<String>,
    available_activities: Vec<String>,
    // Current filter state
    search_projects: bool,
    search_events: bool,

    /// Marks the form with the [`SingleKindFilters`] policy the stylesheet
    /// has to follow.
    single_kind_class: &'static str,
    location_type: String,
    selected_topics: Vec<String>,
    selected_audiences: Vec<String>,
    selected_activities: Vec<String>,
    show_completed: bool,
    show_past: bool,
    event_start: String,
    event_end: String,
    has_classroom_materials: bool,
    has_app: bool,
    is_affiliate: bool,
    usa_only: bool,
    outside_usa: bool,
    exclusively_outdoors: bool,
}

/// The finder URL for `page` of the search `filter_params` describes.
fn finder_page_href(filter_params: &str, page: u32) -> String {
    if filter_params.is_empty() {
        format!("/finder?page={page}")
    } else {
        format!("/finder?page={page}&{filter_params}")
    }
}

const FINDER_PER_PAGE: u32 = 24;

/// Load available filter options from the database
pub(crate) async fn load_filter_options() -> Result<(Vec<String>, Vec<String>, Vec<String>), Error>
{
    let topics = sqlx::query_scalar!("SELECT DISTINCT topic FROM project_topics ORDER BY topic")
        .fetch_all(db())
        .await?;
    let audiences =
        sqlx::query_scalar!("SELECT DISTINCT audience FROM project_audiences ORDER BY audience")
            .fetch_all(db())
            .await?;
    let activities =
        sqlx::query_scalar!("SELECT DISTINCT activity FROM project_activities ORDER BY activity")
            .fetch_all(db())
            .await?;
    Ok((topics, audiences, activities))
}

/// Apply project-specific filters to a QueryBuilder
fn apply_project_filters<'a>(qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>, query: &'a FinderQuery) {
    // A project flagged, marked a duplicate or marked a test is never
    // listed, here or on an exchange; orrery's `find_projects` applies the
    // same rule. Publication is what keeps a submission out until it is
    // approved, so the draft flag -- left set on some long-published
    // projects, and not cleared by exchange approval -- is not consulted.
    qb.push(" AND p.flagged = 0 AND p.duplicate = 0 AND p.is_test = 0");

    // Filter out bad geographic data: (0,0) points and swapped lat/lng
    qb.push(" AND NOT (p.point IS NOT NULL AND ST_X(p.point) = 0 AND ST_Y(p.point) = 0)");
    qb.push(" AND NOT (p.point IS NOT NULL AND ABS(ST_Y(p.point)) > 90)");

    let location_type = query.location_type.as_deref().unwrap_or("either");
    match location_type {
        "remote" => {
            qb.push(" AND p.entirely_online = 1");
        }
        "onsite" => {
            qb.push(" AND p.in_person = 1");
        }
        _ => {}
    }

    let statuses: &[ProjectStatus] = if query.show_completed.is_none() {
        qb.push(" AND (p.end IS NULL OR p.end = '' OR p.end >= datetime('now'))");
        &ProjectStatus::LISTED_BY_FINDER
    } else {
        &ProjectStatus::LISTED_WHEN_INACTIVE
    };
    qb.push(" AND p.status IN (");
    let mut separated = qb.separated(", ");
    for status in statuses {
        separated.push_bind(*status);
    }
    qb.push(")");

    if !query.topic.is_empty() {
        qb.push(" AND p.project_id IN (SELECT project_id FROM project_topics WHERE topic IN (");
        let mut sep = qb.separated(", ");
        for t in &query.topic {
            sep.push_bind(t);
        }
        qb.push("))");
    }

    if !query.audience.is_empty() {
        qb.push(
            " AND p.project_id IN (SELECT project_id FROM project_audiences WHERE audience IN (",
        );
        let mut sep = qb.separated(", ");
        for a in &query.audience {
            sep.push_bind(a);
        }
        qb.push("))");
    }

    if !query.activity.is_empty() {
        qb.push(
            " AND p.project_id IN (SELECT project_id FROM project_activities WHERE activity IN (",
        );
        let mut sep = qb.separated(", ");
        for a in &query.activity {
            sep.push_bind(a);
        }
        qb.push("))");
    }

    let mf = &query.misc_filter;
    if mf.iter().any(|v| v == "has_classroom_materials") {
        qb.push(" AND p.classroom_materials != ''");
    }
    if mf.iter().any(|v| v == "has_app") {
        qb.push(
            " AND (p.mobile_app_url != '' OR p.mobile_app_url_android != '' OR p.mobile_app_url_ios != '')",
        );
    }
    if mf.iter().any(|v| v == "is_affiliate") {
        qb.push(" AND p.is_affiliate = 1");
    }
    if mf.iter().any(|v| v == "usa_only") {
        qb.push(" AND p.country = 'United States'");
    }
    if mf.iter().any(|v| v == "outside_usa") {
        qb.push(" AND p.country != 'United States' AND p.country != ''");
    }
    if mf.iter().any(|v| v == "exclusively_outdoors") {
        qb.push(" AND p.outdoors = 1 AND p.indoors = 0");
    }
}

/// The span of time an event has to touch to match a finder search.
///
/// A search that asks for no dates gets what has not finished yet, which is
/// what the finder shows by default. "Show past events" opens the window to
/// all of time, and a start date the searcher picked replaces the implicit
/// "from now on" floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EventWindow {
    from: Option<EventWindowStart>,
    until: Option<Date>,
}

/// Where a window starts: on a date the searcher picked, or at whatever the
/// present moment is when the query runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventWindowStart {
    Now,
    On(Date),
}

impl EventWindow {
    fn from_query(query: &FinderQuery) -> Self {
        let from = match parse_date(&query.start) {
            Some(date) => Some(EventWindowStart::On(date)),
            None if query.show_past.is_some() => None,
            None => Some(EventWindowStart::Now),
        };

        EventWindow {
            from,
            until: parse_date(&query.end),
        }
    }

    /// Whether the window leaves anything out. An open window is no condition
    /// at all, which also lets an event that has no instances through.
    fn is_open(&self) -> bool {
        self.from.is_none() && self.until.is_none()
    }

    /// Push the conditions an `event_instances` row under `alias` must meet to
    /// fall inside the window, each introduced by its own ` AND `.
    fn push_conditions(&self, qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>, alias: &str) {
        match self.from {
            // An instance that has begun and not yet ended is still going on,
            // so the window keeps it. MAX() also rescues an instance whose
            // stored end precedes its begin, as some imported instances do.
            Some(EventWindowStart::Now) => {
                qb.push(format!(
                    " AND MAX({alias}.begin, {alias}.end) >= datetime('now')"
                ));
            }
            Some(EventWindowStart::On(date)) => {
                qb.push(format!(" AND MAX({alias}.begin, {alias}.end) >= "));
                qb.push_bind(date.to_string());
            }
            None => {}
        }

        if let Some(until) = self.until {
            // Instants carry a time of day and the picked date does not, so
            // the last day asked for runs until the day after it begins.
            qb.push(format!(" AND {alias}.begin < "));
            qb.push_bind(until.tomorrow().unwrap_or(until).to_string());
        }
    }
}

/// A `YYYY-MM-DD` date field, as a date picker submits it. Anything else --
/// including the empty string an untouched picker sends -- is no date at all.
fn parse_date(value: &Option<String>) -> Option<Date> {
    Date::strptime("%Y-%m-%d", value.as_deref()?.trim()).ok()
}

/// Push the scalar subquery for the instance a result is dated and ranked by:
/// the first one inside `window`, or the event's first of all when the window
/// is open. Correlated on the `ev` alias the event subqueries all use.
fn push_selected_instance_begin(
    qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    window: &EventWindow,
    alias: &str,
) {
    qb.push(format!(
        "(SELECT {alias}.begin FROM event_instances {alias} WHERE {alias}.event_id = ev.event_id"
    ));
    window.push_conditions(qb, alias);
    qb.push(format!(" ORDER BY {alias}.begin LIMIT 1)"));
}

/// Push an event's share of `comparison_value` that comes from its date: ten
/// kilometres' worth of rank for every day until the instance the window
/// selected, and none at all for one already under way.
fn push_event_delay(qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>, window: &EventWindow) {
    qb.push("COALESCE(MAX(0.0, julianday(");
    push_selected_instance_begin(qb, window, "ei2");
    qb.push(") - julianday('now')), 0.0) * 10.0");
}

/// Apply event-specific filters to a QueryBuilder
fn apply_event_filters<'a>(qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>, query: &'a FinderQuery) {
    // Filter out bad geographic data: (0,0) points and swapped lat/lng
    qb.push(" AND NOT (ev.point IS NOT NULL AND ST_X(ev.point) = 0 AND ST_Y(ev.point) = 0)");
    qb.push(" AND NOT (ev.point IS NOT NULL AND ABS(ST_Y(ev.point)) > 90)");

    let location_type = query.location_type.as_deref().unwrap_or("either");
    match location_type {
        "remote" => {
            qb.push(" AND ev.entirely_online = 1");
        }
        "onsite" => {
            qb.push(" AND ev.entirely_online = 0");
        }
        _ => {}
    }

    let window = EventWindow::from_query(query);
    if !window.is_open() {
        qb.push(" AND EXISTS (SELECT 1 FROM event_instances ei WHERE ei.event_id = ev.event_id");
        window.push_conditions(qb, "ei");
        qb.push(")");
    }

    if !query.topic.is_empty() {
        qb.push(" AND ev.event_id IN (SELECT event_id FROM event_tags WHERE tag IN (");
        let mut sep = qb.separated(", ");
        for t in &query.topic {
            sep.push_bind(t);
        }
        qb.push("))");
    }
}

/// Convert a description stored in any supported format — Quill Delta JSON,
/// HTML, or Markdown — into plain text, stripping formatting and truncating to
/// `max_len` characters. Descriptions are increasingly stored as Delta, so
/// printing the raw string would show JSON; normalize to Delta and extract a
/// formatting-free excerpt instead. Shared by finder snippets and featured
/// cards so both render Delta descriptions identically.
pub(crate) fn description_excerpt(desc: &str, max_len: usize) -> String {
    match common::quill::to_delta(desc, None) {
        Ok(delta) => common::quill::delta_to_excerpt(&delta, max_len),
        Err(_) => {
            let trimmed = desc.trim();
            if trimmed.chars().count() > max_len {
                format!("{}...", trimmed.chars().take(max_len).collect::<String>())
            } else {
                trimmed.to_string()
            }
        }
    }
}

/// Finder card snippet: a ~200-character plain-text excerpt.
fn make_snippet(desc: &str) -> String {
    description_excerpt(desc, 200)
}

pub(crate) struct FinderSearchResult {
    pub(crate) results: Vec<FinderResult>,
    pub(crate) total: u32,
}

/// Count projects matching text search + filters
fn build_project_count_text<'a>(
    search_term: &'a str,
    fq: &'a FinderQuery,
) -> sqlx::QueryBuilder<sqlx::Sqlite> {
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT COUNT(*) FROM entities_fts f JOIN entities e ON f.slug = e.slug JOIN projects p ON e.target = p.project_id WHERE e.kind = 'Project' AND entities_fts MATCH ",
    );
    qb.push_bind(search_term);
    qb.push(" AND e.publish = 1");
    apply_project_filters(&mut qb, fq);
    qb
}

/// Count events matching text search + filters
fn build_event_count_text<'a>(
    search_term: &'a str,
    fq: &'a FinderQuery,
) -> sqlx::QueryBuilder<sqlx::Sqlite> {
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT COUNT(*) FROM entities_fts f JOIN entities e ON f.slug = e.slug JOIN events ev ON e.target = ev.event_id WHERE e.kind = 'Event' AND entities_fts MATCH ",
    );
    qb.push_bind(search_term);
    qb.push(" AND e.publish = 1");
    apply_event_filters(&mut qb, fq);
    qb
}

/// Search all published entities with no text or location filter, just pagination and sidebar filters
async fn search_all(
    fq: &FinderQuery,
    page: u32,
    kinds: SearchKinds,
) -> Result<FinderSearchResult, Error> {
    let limit = FINDER_PER_PAGE as i64;
    let offset = ((page - 1) * FINDER_PER_PAGE) as i64;
    let window = EventWindow::from_query(fq);

    // Count
    let proj_count: i64 = if kinds.projects() {
        let mut proj_qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(
            "SELECT COUNT(*) FROM entities e JOIN projects p ON e.target = p.project_id WHERE e.kind = 'Project' AND e.publish = 1",
        );
        apply_project_filters(&mut proj_qb, fq);
        proj_qb.build_query_scalar().fetch_one(db()).await?
    } else {
        0
    };

    let evt_count: i64 = if kinds.events() {
        let mut evt_qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(
            "SELECT COUNT(*) FROM entities e JOIN events ev ON e.target = ev.event_id WHERE e.kind = 'Event' AND e.publish = 1",
        );
        apply_event_filters(&mut evt_qb, fq);
        evt_qb.build_query_scalar().fetch_one(db()).await?
    } else {
        0
    };
    let total = (proj_count + evt_count) as u32;

    // Build UNION ALL query
    let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new("SELECT * FROM (");

    // Project subquery
    if kinds.projects() {
        qb.push(
            "SELECT e.slug, 'Project' AS entity_type, p.name AS title, p.description, p.image, \
             p.goal, p.task, p.status, \
             CASE WHEN p.entirely_online = 1 THEN 'Online' \
                  WHEN p.city != '' AND p.state != '' THEN p.city || ', ' || p.state \
                  WHEN p.location_name != '' THEN p.location_name \
                  ELSE '' END AS location_display, \
             NULL AS event_begin, \
             CASE WHEN p.regions IS NOT NULL THEN AsGeoJSON(p.regions) \
                  WHEN p.point IS NOT NULL THEN AsGeoJSON(p.point) \
                  ELSE NULL END AS geo_json, \
             0.0 AS comparison_value \
             FROM entities e JOIN projects p ON e.target = p.project_id \
             WHERE e.kind = 'Project' AND e.publish = 1",
        );
        apply_project_filters(&mut qb, fq);
    }

    if kinds.projects() && kinds.events() {
        qb.push(" UNION ALL ");
    }

    // Event subquery
    if kinds.events() {
        qb.push(
            "SELECT e.slug, 'Event' AS entity_type, ev.name AS title, ev.description, ev.image, \
             '' AS goal, '' AS task, '' AS status, \
             CASE WHEN ev.entirely_online = 1 THEN 'Online' \
                  WHEN ev.city != '' AND ev.state != '' THEN ev.city || ', ' || ev.state \
                  WHEN ev.location_name != '' THEN ev.location_name \
                  ELSE '' END AS location_display, ",
        );
        push_selected_instance_begin(&mut qb, &window, "ei");
        qb.push(
            " AS event_begin, \
             CASE WHEN ev.regions IS NOT NULL THEN AsGeoJSON(ev.regions) \
                  WHEN ev.point IS NOT NULL THEN AsGeoJSON(ev.point) \
                  ELSE NULL END AS geo_json, ",
        );
        push_event_delay(&mut qb, &window);
        qb.push(
            " * 1.0 AS comparison_value \
             FROM entities e JOIN events ev ON e.target = ev.event_id \
             WHERE e.kind = 'Event' AND e.publish = 1",
        );
        apply_event_filters(&mut qb, fq);
    }

    qb.push(") ORDER BY comparison_value LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows = qb
        .build_query_as::<(
            String,         // slug
            String,         // entity_type
            String,         // title
            String,         // description
            String,         // image
            String,         // goal
            String,         // task
            String,         // status
            String,         // location_display
            Option<String>, // event_begin
            Option<String>, // geo_json
            f64,            // comparison_value
        )>()
        .fetch_all(db())
        .await?;

    let results = rows
        .into_iter()
        .map(|row| FinderResult {
            slug: row.0,
            entity_type: row.1,
            title: row.2,
            description_snippet: make_snippet(&row.3),
            image: row.4,
            goal: row.5,
            task: row.6,
            is_completed: row.7 == "Complete",
            location_display: row.8,
            event_begin: row.9,
            distance_km: None,
            geo_json: row.10,
        })
        .collect();

    Ok(FinderSearchResult { results, total })
}

async fn search_text_only(
    fq: &FinderQuery,
    page: u32,
    style: FtsTermStyle,
    kinds: SearchKinds,
) -> Result<FinderSearchResult, Error> {
    let q = fq.q.as_deref().unwrap_or("");
    let search_term = style.term_for(q);
    let limit = FINDER_PER_PAGE as i64;
    let offset = ((page - 1) * FINDER_PER_PAGE) as i64;
    let window = EventWindow::from_query(fq);

    // Count
    let proj_count: i64 = if kinds.projects() {
        build_project_count_text(&search_term, fq)
            .build_query_scalar()
            .fetch_one(db())
            .await?
    } else {
        0
    };
    let evt_count: i64 = if kinds.events() {
        build_event_count_text(&search_term, fq)
            .build_query_scalar()
            .fetch_one(db())
            .await?
    } else {
        0
    };
    let total = (proj_count + evt_count) as u32;

    // Build UNION ALL query
    let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new("SELECT * FROM (");

    // Project subquery
    if kinds.projects() {
        qb.push(
            "SELECT f.slug, 'Project' AS entity_type, f.title, p.description, p.image, \
             p.goal, p.task, p.status, \
             CASE WHEN p.entirely_online = 1 THEN 'Online' \
                  WHEN p.city != '' AND p.state != '' THEN p.city || ', ' || p.state \
                  WHEN p.location_name != '' THEN p.location_name \
                  ELSE '' END AS location_display, \
             NULL AS event_begin, \
             CASE WHEN p.regions IS NOT NULL THEN AsGeoJSON(p.regions) \
                  WHEN p.point IS NOT NULL THEN AsGeoJSON(p.point) \
                  ELSE NULL END AS geo_json, \
             0.0 AS comparison_value \
             FROM entities_fts f \
             JOIN entities e ON f.slug = e.slug \
             JOIN projects p ON e.target = p.project_id \
             WHERE e.kind = 'Project' AND entities_fts MATCH",
        );
        qb.push_bind(search_term.clone());
        qb.push(" AND e.publish = 1");
        apply_project_filters(&mut qb, fq);
    }

    if kinds.projects() && kinds.events() {
        qb.push(" UNION ALL ");
    }

    // Event subquery
    if kinds.events() {
        qb.push(
            "SELECT f.slug, 'Event' AS entity_type, f.title, ev.description, ev.image, \
             '' AS goal, '' AS task, '' AS status, \
             CASE WHEN ev.entirely_online = 1 THEN 'Online' \
                  WHEN ev.city != '' AND ev.state != '' THEN ev.city || ', ' || ev.state \
                  WHEN ev.location_name != '' THEN ev.location_name \
                  ELSE '' END AS location_display, ",
        );
        push_selected_instance_begin(&mut qb, &window, "ei");
        qb.push(
            " AS event_begin, \
             CASE WHEN ev.regions IS NOT NULL THEN AsGeoJSON(ev.regions) \
                  WHEN ev.point IS NOT NULL THEN AsGeoJSON(ev.point) \
                  ELSE NULL END AS geo_json, ",
        );
        push_event_delay(&mut qb, &window);
        qb.push(
            " * 1.0 AS comparison_value \
             FROM entities_fts f \
             JOIN entities e ON f.slug = e.slug \
             JOIN events ev ON e.target = ev.event_id \
             WHERE e.kind = 'Event' AND entities_fts MATCH ",
        );
        qb.push_bind(search_term.clone());
        qb.push(" AND e.publish = 1");
        apply_event_filters(&mut qb, fq);
    }

    qb.push(") ORDER BY comparison_value LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows = qb
        .build_query_as::<(
            String,         // slug
            String,         // entity_type
            Option<String>, // title
            String,         // description
            String,         // image
            String,         // goal
            String,         // task
            String,         // status
            String,         // location_display
            Option<String>, // event_begin
            Option<String>, // geo_json
            f64,            // comparison_value
        )>()
        .fetch_all(db())
        .await?;

    let results = rows
        .into_iter()
        .map(|row| FinderResult {
            slug: row.0,
            entity_type: row.1,
            title: row.2.unwrap_or_default(),
            description_snippet: make_snippet(&row.3),
            image: row.4,
            goal: row.5,
            task: row.6,
            is_completed: row.7 == "Complete",
            location_display: row.8,
            event_begin: row.9,
            distance_km: None,
            geo_json: row.10,
        })
        .collect();

    Ok(FinderSearchResult { results, total })
}

async fn search_with_location(
    fq: &FinderQuery,
    lat: f64,
    lng: f64,
    page: u32,
    style: FtsTermStyle,
    kinds: SearchKinds,
) -> Result<FinderSearchResult, Error> {
    let q = fq.q.as_deref().unwrap_or("");
    let search_term = style.term_for(q);

    // Count
    let proj_count: i64 = if kinds.projects() {
        build_project_count_text(&search_term, fq)
            .build_query_scalar()
            .fetch_one(db())
            .await?
    } else {
        0
    };
    let evt_count: i64 = if kinds.events() {
        build_event_count_text(&search_term, fq)
            .build_query_scalar()
            .fetch_one(db())
            .await?
    } else {
        0
    };
    let total = (proj_count + evt_count) as u32;

    let limit = FINDER_PER_PAGE as i64;
    let offset = ((page - 1) * FINDER_PER_PAGE) as i64;
    let window = EventWindow::from_query(fq);

    // Build UNION ALL with spatial columns
    let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new("SELECT * FROM (");

    // Project subquery
    if kinds.projects() {
        qb.push(
            "SELECT f.slug, 'Project' AS entity_type, f.title, p.description, p.image, \
         p.goal, p.task, p.status, \
         CASE WHEN p.entirely_online = 1 THEN 'Online' \
              WHEN p.city != '' AND p.state != '' THEN p.city || ', ' || p.state \
              WHEN p.location_name != '' THEN p.location_name \
              ELSE '' END AS location_display, \
         NULL AS event_begin, ",
        );
        push_distance_km(&mut qb, LocatedTable::Projects, lat, lng);
        qb.push(
            ", \
         CASE WHEN p.regions IS NOT NULL THEN AsGeoJSON(p.regions) \
              WHEN p.point IS NOT NULL THEN AsGeoJSON(p.point) \
              ELSE NULL END AS geo_json, ",
        );
        push_project_comparison(&mut qb, lat, lng);
        qb.push(
            " \
         FROM entities_fts f \
         JOIN entities e ON f.slug = e.slug \
         JOIN projects p ON e.target = p.project_id \
         WHERE e.kind = 'Project' AND entities_fts MATCH ",
        );
        qb.push_bind(search_term.clone());
        qb.push(" AND e.publish = 1");
        apply_project_filters(&mut qb, fq);
        qb.push(" GROUP BY f.slug");
    }

    if kinds.projects() && kinds.events() {
        qb.push(" UNION ALL ");
    }

    // Event subquery
    if kinds.events() {
        qb.push(
            "SELECT f.slug, 'Event' AS entity_type, f.title, ev.description, ev.image, \
         '' AS goal, '' AS task, '' AS status, \
         CASE WHEN ev.entirely_online = 1 THEN 'Online' \
              WHEN ev.city != '' AND ev.state != '' THEN ev.city || ', ' || ev.state \
              WHEN ev.location_name != '' THEN ev.location_name \
              ELSE '' END AS location_display, ",
        );
        push_selected_instance_begin(&mut qb, &window, "ei");
        qb.push(" AS event_begin, ");
        push_distance_km(&mut qb, LocatedTable::Events, lat, lng);
        qb.push(
            ", \
         CASE WHEN ev.regions IS NOT NULL THEN AsGeoJSON(ev.regions) \
              WHEN ev.point IS NOT NULL THEN AsGeoJSON(ev.point) \
              ELSE NULL END AS geo_json, ",
        );
        push_event_comparison(&mut qb, lat, lng, &window);
        qb.push(
            " \
         FROM entities_fts f \
         JOIN entities e ON f.slug = e.slug \
         JOIN events ev ON e.target = ev.event_id \
         WHERE e.kind = 'Event' AND entities_fts MATCH",
        );
        qb.push_bind(search_term.clone());
        qb.push(" AND e.publish = 1");
        apply_event_filters(&mut qb, fq);
        qb.push(" GROUP BY f.slug");
    }

    qb.push(") ORDER BY comparison_value LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows = qb
        .build_query_as::<(
            String,         // slug
            String,         // entity_type
            Option<String>, // title
            String,         // description
            String,         // image
            String,         // goal
            String,         // task
            String,         // status
            String,         // location_display
            Option<String>, // event_begin
            Option<f64>,    // distance_km
            Option<String>, // geo_json
            f64,            // comparison_value
        )>()
        .fetch_all(db())
        .await?;

    let results = rows
        .into_iter()
        .map(|row| FinderResult {
            slug: row.0,
            entity_type: row.1,
            title: row.2.unwrap_or_default(),
            description_snippet: make_snippet(&row.3),
            image: row.4,
            goal: row.5,
            task: row.6,
            is_completed: row.7 == "Complete",
            location_display: row.8,
            event_begin: row.9,
            distance_km: row.10,
            geo_json: row.11,
        })
        .collect();

    Ok(FinderSearchResult { results, total })
}

/// Count projects matching location-only + filters
fn build_project_count_location<'a>(fq: &'a FinderQuery) -> sqlx::QueryBuilder<sqlx::Sqlite> {
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT COUNT(*) FROM entities e JOIN projects p ON e.target = p.project_id WHERE e.kind = 'Project' AND e.publish = 1 AND (p.point IS NOT NULL OR p.regions IS NOT NULL OR p.entirely_online = 1)",
    );
    apply_project_filters(&mut qb, fq);
    qb
}

/// Count events matching location-only + filters
fn build_event_count_location<'a>(fq: &'a FinderQuery) -> sqlx::QueryBuilder<sqlx::Sqlite> {
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT COUNT(*) FROM entities e JOIN events ev ON e.target = ev.event_id WHERE e.kind = 'Event' AND e.publish = 1 AND (ev.point IS NOT NULL OR ev.regions IS NOT NULL OR ev.entirely_online = 1)",
    );
    apply_event_filters(&mut qb, fq);
    qb
}

/// The 13-column row shape produced by the location-ranked project/event
/// subqueries. Shared so the finder's location-only search and the home-page
/// [`nearby_entities`] list decode identically.
type LocationRow = (
    String,         // slug
    String,         // entity_type
    String,         // title
    String,         // description
    String,         // image
    String,         // goal
    String,         // task
    String,         // status
    String,         // location_display
    Option<String>, // event_begin
    Option<f64>,    // distance_km
    Option<String>, // geo_json
    f64,            // comparison_value
);

/// Map a [`LocationRow`] into a [`FinderResult`].
fn location_row_to_result(row: LocationRow) -> FinderResult {
    FinderResult {
        slug: row.0,
        entity_type: row.1,
        title: row.2,
        description_snippet: make_snippet(&row.3),
        image: row.4,
        goal: row.5,
        task: row.6,
        is_completed: row.7 == "Complete",
        location_display: row.8,
        event_begin: row.9,
        distance_km: row.10,
        geo_json: row.11,
    }
}

/// The ranking distance, in kilometres, that stands for "anywhere on Earth".
///
/// Roughly the square root of the Earth's surface area in square kilometres,
/// so a footprint with no measurable size ranks as if it covered the planet,
/// and one with no measurable distance as if it were as far away as that.
const WHOLE_WORLD_KM: f64 = 22583.0;

/// SQL that is true when the `regions` footprint in `table` can be measured on the
/// ellipsoid.
///
/// `ST_Distance(.., 1)` and `ST_Area(.., 1)` join each pair of vertices by
/// the shorter geodesic, while the stored coordinates describe the polygon on
/// the flat longitude/latitude plane. The two readings agree only while no
/// polygon part spans half the globe: a part that wide is either the whole
/// world or a ring drawn the long way round, and measures as some other
/// shape. The test is per part because a region crossing the antimeridian is
/// stored as parts either side of it, as New Zealand, Alaska and the United
/// States are, and each of those parts measures correctly.
///
/// Orrery's `measurable_as_geography` makes the same per-part test. SpatiaLite
/// additionally needs every longitude within +/-180: PostGIS wraps a vertex
/// drawn past 180 degrees back onto the globe, but SpatiaLite's distance does
/// not, so a box drawn from 170 to 190 degrees measures hundreds of
/// kilometres too far from anyone just east of the antimeridian.
///
/// A footprint that fails ranks by the project's point if it has one, and
/// otherwise as covering the whole world.
fn measurable_footprint(table: LocatedTable) -> String {
    let table = table.alias();

    format!(
        "({table}.regions IS NOT NULL \
          AND MbrMinX({table}.regions) >= -180 AND MbrMaxX({table}.regions) <= 180 \
          AND NOT EXISTS ( \
            WITH RECURSIVE part(n) AS ( \
              SELECT 1 UNION ALL SELECT n + 1 FROM part WHERE n < NumGeometries({table}.regions) \
            ) \
            SELECT 1 FROM part \
            WHERE MbrMaxX(GeometryN({table}.regions, n)) - MbrMinX(GeometryN({table}.regions, n)) >= 180 \
          ))"
    )
}

/// Which kind of entity a location-ranked subquery reads, by the alias its
/// table goes under there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocatedTable {
    /// `projects p`
    Projects,
    /// `events ev`
    Events,
}

impl LocatedTable {
    fn alias(self) -> &'static str {
        match self {
            LocatedTable::Projects => "p",
            LocatedTable::Events => "ev",
        }
    }
}

/// Push the searcher's location as a SpatiaLite point.
fn push_search_point(qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>, lat: f64, lng: f64) {
    qb.push("MakePoint(");
    qb.push_bind(lng);
    qb.push(", ");
    qb.push_bind(lat);
    qb.push(", 4326)");
}

/// Push the `distance_km` column shown beside a result: from the searcher to
/// the footprint if it is measurable, else to the point, else nothing.
fn push_distance_km(
    qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    table: LocatedTable,
    lat: f64,
    lng: f64,
) {
    let measurable = measurable_footprint(table);
    let table = table.alias();

    qb.push(format!(
        "CASE WHEN {table}.entirely_online = 1 THEN NULL \
              WHEN {measurable} THEN ST_Distance(ST_MakeValid({table}.regions), "
    ));
    push_search_point(qb, lat, lng);
    qb.push(format!(
        ", 1) / 1000.0 WHEN {table}.point IS NOT NULL THEN ST_Distance({table}.point, "
    ));
    push_search_point(qb, lat, lng);
    qb.push(", 1) / 1000.0 ELSE NULL END AS distance_km");
}

/// Push a project's `comparison_value`, the key results sort by: a footprint
/// ranks by its distance plus the square root of its area, so a small area
/// nearby beats a large one that merely reaches this far; a point by plain
/// distance; an online project as if 200 km away.
fn push_project_comparison(qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>, lat: f64, lng: f64) {
    let measurable = measurable_footprint(LocatedTable::Projects);

    qb.push(format!(
        "CASE WHEN p.entirely_online = 1 THEN 200.0 \
              WHEN {measurable} THEN COALESCE(ST_Distance(ST_MakeValid(p.regions), "
    ));
    push_search_point(qb, lat, lng);
    qb.push(format!(
        ", 1) / 1000.0, 0) + COALESCE(NULLIF(SQRT(ST_Area(ST_MakeValid(p.regions), 1) / 1000000.0), 0.0), {WHOLE_WORLD_KM:.1}) \
              WHEN p.point IS NOT NULL THEN COALESCE(ST_Distance(p.point, "
    ));
    push_search_point(qb, lat, lng);
    qb.push(format!(
        ", 1) / 1000.0, {WHOLE_WORLD_KM:.1}) \
              WHEN p.regions IS NOT NULL THEN {WHOLE_WORLD_KM:.1} \
              ELSE 0.0 END * 1.0 AS comparison_value"
    ));
}

/// Push an event's `comparison_value`: [`push_event_delay`]'s ten kilometres
/// per day until the instance the window selected, plus its distance -- to a
/// footprint, else to its point, with an online event counted as 100 km away.
fn push_event_comparison(
    qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    lat: f64,
    lng: f64,
    window: &EventWindow,
) {
    let measurable = measurable_footprint(LocatedTable::Events);

    push_event_delay(qb, window);
    qb.push(format!(
        " + CASE WHEN ev.entirely_online = 1 THEN 100.0 \
                WHEN {measurable} THEN COALESCE(ST_Distance(ST_MakeValid(ev.regions), "
    ));
    push_search_point(qb, lat, lng);
    qb.push(
        ", 1) / 1000.0, 0) \
                WHEN ev.point IS NOT NULL THEN COALESCE(ST_Distance(ev.point, ",
    );
    push_search_point(qb, lat, lng);
    qb.push(format!(
        ", 1) / 1000.0, {WHOLE_WORLD_KM:.1}) \
                WHEN ev.regions IS NOT NULL THEN {WHOLE_WORLD_KM:.1} \
                ELSE 0.0 END * 1.0 AS comparison_value"
    ));
}

/// Push the location-ranked **project** subquery into `qb` (no filters, no outer
/// wrapper). Shared by [`search_location_only`] and [`nearby_entities`].
/// The distance and `comparison_value` expressions come from
/// [`push_distance_km`] and [`push_project_comparison`], which the text
/// search uses too. Callers append their own filters and close the statement.
fn push_project_location_subquery(qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>, lat: f64, lng: f64) {
    qb.push(
        "SELECT e.slug, 'Project' AS entity_type, p.name AS title, p.description, p.image, \
         p.goal, p.task, p.status, \
         CASE WHEN p.entirely_online = 1 THEN 'Online' \
              WHEN p.city != '' AND p.state != '' THEN p.city || ', ' || p.state \
              WHEN p.location_name != '' THEN p.location_name \
              ELSE '' END AS location_display, \
         NULL AS event_begin, ",
    );
    push_distance_km(qb, LocatedTable::Projects, lat, lng);
    qb.push(
        ", \
         CASE WHEN p.regions IS NOT NULL THEN AsGeoJSON(p.regions) \
              WHEN p.point IS NOT NULL THEN AsGeoJSON(p.point) \
              ELSE NULL END AS geo_json, ",
    );
    push_project_comparison(qb, lat, lng);
    qb.push(
        " \
         FROM entities e JOIN projects p ON e.target = p.project_id \
         WHERE e.kind = 'Project' AND e.publish = 1 \
         AND (p.point IS NOT NULL OR p.regions IS NOT NULL OR p.entirely_online = 1)",
    );
}

/// Push the location-ranked **event** subquery into `qb` (no filters, no outer
/// wrapper). Companion to [`push_project_location_subquery`].
fn push_event_location_subquery(
    qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    lat: f64,
    lng: f64,
    window: &EventWindow,
) {
    qb.push(
        "SELECT e.slug, 'Event' AS entity_type, ev.name AS title, ev.description, ev.image, \
         '' AS goal, '' AS task, '' AS status, \
         CASE WHEN ev.entirely_online = 1 THEN 'Online' \
              WHEN ev.city != '' AND ev.state != '' THEN ev.city || ', ' || ev.state \
              WHEN ev.location_name != '' THEN ev.location_name \
              ELSE '' END AS location_display, ",
    );
    push_selected_instance_begin(qb, window, "ei");
    qb.push(" AS event_begin, ");
    push_distance_km(qb, LocatedTable::Events, lat, lng);
    qb.push(
        ", \
         CASE WHEN ev.regions IS NOT NULL THEN AsGeoJSON(ev.regions) \
              WHEN ev.point IS NOT NULL THEN AsGeoJSON(ev.point) \
              ELSE NULL END AS geo_json, ",
    );
    push_event_comparison(qb, lat, lng, window);
    qb.push(
        " \
         FROM entities e JOIN events ev ON e.target = ev.event_id \
         WHERE e.kind = 'Event' AND e.publish = 1 \
         AND (ev.point IS NOT NULL OR ev.regions IS NOT NULL OR ev.entirely_online = 1)",
    );
}

/// Push the full location-ranked projects∪events body — both subqueries with
/// their finder filters applied — into `qb`, without the outer wrapper or
/// `ORDER BY`. Shared by [`search_location_only`] and [`nearby_entities`] so the
/// home-page "near you" list and the finder's location search filter and rank
/// identically.
fn push_location_union(
    qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    fq: &FinderQuery,
    lat: f64,
    lng: f64,
    kinds: SearchKinds,
) {
    if kinds.projects() {
        push_project_location_subquery(qb, lat, lng);
        apply_project_filters(qb, fq);
    }
    if kinds.projects() && kinds.events() {
        qb.push(" UNION ALL ");
    }
    if kinds.events() {
        push_event_location_subquery(qb, lat, lng, &EventWindow::from_query(fq));
        apply_event_filters(qb, fq);
    }
}

/// The finder query the home page's "see all" link produces: a bare
/// location-only search (lat/lng set, no other filters). `location_name` is
/// display-only and does not affect results, so it is left unset here.
fn default_location_query(lat: f64, lng: f64) -> FinderQuery {
    FinderQuery {
        q: None,
        lat: Some(lat),
        lng: Some(lng),
        location_name: None,
        page: None,
        location_type: None,
        topic: Vec::new(),
        audience: Vec::new(),
        activity: Vec::new(),
        show_completed: None,
        misc_filter: Vec::new(),
        kind: Vec::new(),
        start: None,
        end: None,
        show_past: None,
    }
}

/// Up to `limit` published projects and events nearest to `(lat, lng)` — the
/// top of the finder's location-only search for the same point. Uses the same
/// default query, filters, and `comparison_value` ranking as
/// [`search_location_only`], so the home-page "near you" list and the finder's
/// "see all" results agree.
pub async fn nearby_entities(lat: f64, lng: f64, limit: i64) -> Result<Vec<FinderResult>, Error> {
    let fq = default_location_query(lat, lng);

    let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new("SELECT * FROM (");
    push_location_union(&mut qb, &fq, lat, lng, SearchKinds::Both);
    qb.push(") ORDER BY comparison_value LIMIT ");
    qb.push_bind(limit.max(0));

    let rows = qb.build_query_as::<LocationRow>().fetch_all(db()).await?;
    Ok(rows.into_iter().map(location_row_to_result).collect())
}

async fn search_location_only(
    fq: &FinderQuery,
    lat: f64,
    lng: f64,
    page: u32,
    kinds: SearchKinds,
) -> Result<FinderSearchResult, Error> {
    // Count
    let proj_count: i64 = if kinds.projects() {
        build_project_count_location(fq)
            .build_query_scalar()
            .fetch_one(db())
            .await?
    } else {
        0
    };
    let evt_count: i64 = if kinds.events() {
        build_event_count_location(fq)
            .build_query_scalar()
            .fetch_one(db())
            .await?
    } else {
        0
    };
    let total = (proj_count + evt_count) as u32;

    let limit = FINDER_PER_PAGE as i64;
    let offset = ((page - 1) * FINDER_PER_PAGE) as i64;

    let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new("SELECT * FROM (");

    push_location_union(&mut qb, fq, lat, lng, kinds);

    qb.push(") ORDER BY comparison_value LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows = qb.build_query_as::<LocationRow>().fetch_all(db()).await?;

    let results = rows.into_iter().map(location_row_to_result).collect();

    Ok(FinderSearchResult { results, total })
}

/// Run one page of a finder search using the same strategy selection as the
/// `/finder` page: text + location, location only, text only, or browse-all,
/// with the query's facet filters applied in every case.
pub(crate) async fn run_finder_search(
    query: &FinderQuery,
    page: u32,
    style: FtsTermStyle,
    kinds: SearchKinds,
) -> Result<FinderSearchResult, Error> {
    if matches!(kinds, SearchKinds::Neither) {
        return Ok(FinderSearchResult {
            results: Vec::new(),
            total: 0,
        });
    }

    let q = query.q.as_deref().unwrap_or("").trim();
    let has_query = !q.is_empty();

    match (query.lat, query.lng) {
        (Some(lat), Some(lng)) if has_query => {
            search_with_location(query, lat, lng, page, style, kinds).await
        }
        (Some(lat), Some(lng)) => search_location_only(query, lat, lng, page, kinds).await,
        _ if has_query => search_text_only(query, page, style, kinds).await,
        _ => search_all(query, page, kinds).await,
    }
}

#[handler]
pub async fn handle_finder(
    _session: &Session,
    QsQuery(query): QsQuery<FinderQuery>,
    Authenticated(user_info): Authenticated,
    device: Device,
    interaction: Interaction,
) -> Result<Response, poem::Error> {
    let q = query.q.as_deref().unwrap_or("").trim().to_string();
    let lat = query.lat;
    let lng = query.lng;
    let location_name = query.location_name.clone().unwrap_or_default();
    let page = query.page.unwrap_or(1).max(1);

    let has_location = lat.is_some() && lng.is_some();
    let has_query = !q.is_empty();

    let (available_topics, available_audiences, available_activities) =
        load_filter_options().await.map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    let kinds = SearchKinds::from_query(&query);

    // What the sidebar is offering, which is what the results answer. The
    // untouched `query` still describes the form, and the links below carry
    // it, so a filter that is out of scope here is waiting where the searcher
    // left it when they narrow the search back to its kind.
    let searched = query.as_offered(kinds, SingleKindFilters::CURRENT);

    let search_result = run_finder_search(&searched, page, FtsTermStyle::Phrase, kinds)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Finder search error: {}", e);
            FinderSearchResult {
                results: Vec::new(),
                total: 0,
            }
        });

    // Log search event
    {
        use common::message::log::SearchEvent;
        use common::service::send;
        use common::timestring::TimeString;

        // The filters that shaped `result_count`, rather than the ones the form
        // carried past them.
        let search_type = match (has_query, has_location) {
            (true, true) => "text_location",
            (false, true) => "location_only",
            (true, false) => "text_only",
            (false, false) => "browse_all",
        };

        let event = SearchEvent {
            when: TimeString::now().to_string(),
            device_id: device.as_ref().to_string(),
            interaction_id: interaction.as_ref().to_string(),
            user_id: user_info.as_ref().map(|u| u.user_id.to_string()),
            query_text: if q.is_empty() { None } else { Some(q.clone()) },
            lat: query.lat,
            lng: query.lng,
            location_name: if query.location_name.as_deref().unwrap_or("").is_empty() {
                None
            } else {
                query.location_name.clone()
            },
            location_type: searched.location_type.clone(),
            topics: if searched.topic.is_empty() {
                None
            } else {
                serde_json::to_string(&searched.topic).ok()
            },
            audiences: if searched.audience.is_empty() {
                None
            } else {
                serde_json::to_string(&searched.audience).ok()
            },
            activities: if searched.activity.is_empty() {
                None
            } else {
                serde_json::to_string(&searched.activity).ok()
            },
            misc_filters: if searched.misc_filter.is_empty() {
                None
            } else {
                serde_json::to_string(&searched.misc_filter).ok()
            },
            show_completed: searched.show_completed.is_some(),
            kinds: Some(kinds.as_params().join(",")),
            show_past: searched.show_past.is_some(),
            event_start: parse_date(&searched.start).map(|date| date.to_string()),
            event_end: parse_date(&searched.end).map(|date| date.to_string()),
            page: page as i32,
            result_count: search_result.total as i32,
            search_type: search_type.to_string(),
        };

        tokio::spawn(async move {
            if let Err(e) = send("log.search_event", event).await {
                tracing::warn!("Failed to log search event: {}", e);
            }
        });
    }

    let total_pages = if search_result.total == 0 {
        0
    } else {
        (search_result.total + FINDER_PER_PAGE - 1) / FINDER_PER_PAGE
    };

    let location_type = query
        .location_type
        .as_deref()
        .unwrap_or("either")
        .to_string();

    let filter_params = query.to_query_string();

    let lat_str = lat.map(|v| v.to_string()).unwrap_or_default();
    let lng_str = lng.map(|v| v.to_string()).unwrap_or_default();

    let layout = StandardLayout::new(
        "/finder",
        FinderContent {
            results: search_result.results,
            query: q,
            location_name,
            lat: lat_str,
            lng: lng_str,
            geosearch_key: env!("GEOSEARCH_KEY").to_string(),
            pagination: NumberedPagination::new(page, total_pages, |p| {
                finder_page_href(&filter_params, p)
            }),
            available_topics,
            available_audiences,
            available_activities,
            search_projects: kinds.projects(),
            search_events: kinds.events(),
            single_kind_class: SingleKindFilters::CURRENT.form_class(),
            location_type,
            selected_topics: query.topic.clone(),
            selected_audiences: query.audience.clone(),
            selected_activities: query.activity.clone(),
            show_completed: query.show_completed.is_some(),
            show_past: query.show_past.is_some(),
            event_start: parse_date(&query.start)
                .map(|date| date.to_string())
                .unwrap_or_default(),
            event_end: parse_date(&query.end)
                .map(|date| date.to_string())
                .unwrap_or_default(),
            has_classroom_materials: query
                .misc_filter
                .iter()
                .any(|v| v == "has_classroom_materials"),
            has_app: query.misc_filter.iter().any(|v| v == "has_app"),
            is_affiliate: query.misc_filter.iter().any(|v| v == "is_affiliate"),
            usa_only: query.misc_filter.iter().any(|v| v == "usa_only"),
            outside_usa: query.misc_filter.iter().any(|v| v == "outside_usa"),
            exclusively_outdoors: query
                .misc_filter
                .iter()
                .any(|v| v == "exclusively_outdoors"),
        },
    )
    .prepare(user_info.clone())
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    FinderPage {
        head: Head::default()
            .with_geosearch()
            .with_maplibre()
            .prepare(None)
            .await
            .map_err(|e| {
                poem::Error::from_string(
                    e.to_string(),
                    poem::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
            })?,
        layout,
    }
    .into_response(None)
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        EVENT_KIND, EventWindow, EventWindowStart, FinderQuery, LocatedTable, PROJECT_KIND,
        SearchKinds, SingleKindFilters, WHOLE_WORLD_KM, apply_event_filters,
        default_location_query, make_snippet, push_distance_km, push_event_comparison,
        push_project_comparison,
    };
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

    /// Crosses the antimeridian the way stored regions do: one part either
    /// side of it, each ten degrees wide.
    const SPLIT: &str = "MULTIPOLYGON(((170 -40, 180 -40, 180 -30, 170 -30, 170 -40)), \
                         ((-180 -40, -170 -40, -170 -30, -180 -30, -180 -40)))";

    /// The same box drawn as one ring the long way round.
    const LONG_WAY: &str = "MULTIPOLYGON(((-170 -40, 170 -40, 170 -30, -170 -30, -170 -40)))";

    /// The same box drawn as one ring past 180 degrees.
    const UNWRAPPED: &str = "MULTIPOLYGON(((170 -40, 190 -40, 190 -30, 170 -30, 170 -40)))";

    const WORLD: &str = "MULTIPOLYGON(((-180 -90, 180 -90, 180 90, -180 90, -180 -90)))";

    /// What the finder asks for when nobody has touched the date fields.
    const UPCOMING: EventWindow = EventWindow {
        from: Some(EventWindowStart::Now),
        until: None,
    };

    /// Where the searcher stands: inside the eastern part of [`SPLIT`].
    const LAT: f64 = -35.0;
    const LNG: f64 = -175.0;

    /// A point about 111 km due north of the searcher.
    const POINT_NORTH: &str = "POINT(-175 -34)";

    /// A scratch database with just the columns the ranking reads.
    async fn database() -> SqlitePool {
        // SAFETY: mod_spatialite is a trusted extension, the same one the
        // application loads.
        let options = unsafe { SqliteConnectOptions::new().extension("mod_spatialite") };

        // One connection, because every connection to ":memory:" is a
        // database of its own.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.filename(":memory:"))
            .await
            .unwrap();

        // Ellipsoidal measurement looks the SRID up in spatial_ref_sys.
        for statement in [
            "SELECT InitSpatialMetadata(1)",
            "CREATE TABLE projects (name TEXT, entirely_online INTEGER, regions BLOB, point BLOB)",
            "CREATE TABLE events (event_id TEXT, name TEXT, entirely_online INTEGER, regions BLOB, point BLOB)",
            "CREATE TABLE event_instances (event_id TEXT, begin TEXT, \"end\" TEXT)",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }

        pool
    }

    async fn add(
        pool: &SqlitePool,
        table: LocatedTable,
        name: &str,
        regions: Option<&str>,
        point: Option<&str>,
    ) {
        let query = match table {
            LocatedTable::Projects => sqlx::query(
                "INSERT INTO projects VALUES (?, 0, GeomFromText(?, 4326), GeomFromText(?, 4326))",
            )
            .bind(name),
            LocatedTable::Events => sqlx::query(
                "INSERT INTO events VALUES (?, ?, 0, GeomFromText(?, 4326), GeomFromText(?, 4326))",
            )
            .bind(name)
            .bind(name),
        };

        query.bind(regions).bind(point).execute(pool).await.unwrap();
    }

    /// Each row's name, `distance_km` and `comparison_value`.
    async fn ranked(pool: &SqlitePool, table: LocatedTable) -> Vec<(String, Option<f64>, f64)> {
        let alias = table.alias();

        let mut qb = sqlx::QueryBuilder::new(format!("SELECT {alias}.name, "));
        push_distance_km(&mut qb, table, LAT, LNG);
        qb.push(", ");
        let from = match table {
            LocatedTable::Projects => {
                push_project_comparison(&mut qb, LAT, LNG);
                "projects"
            }
            LocatedTable::Events => {
                push_event_comparison(&mut qb, LAT, LNG, &UPCOMING);
                "events"
            }
        };
        qb.push(format!(" FROM {from} {alias}"));

        qb.build_query_as::<(String, Option<f64>, f64)>()
            .fetch_all(pool)
            .await
            .unwrap()
    }

    fn row<'a>(
        rows: &'a [(String, Option<f64>, f64)],
        name: &str,
    ) -> &'a (String, Option<f64>, f64) {
        rows.iter().find(|r| r.0 == name).unwrap()
    }

    /// A search of the events, with the date fields filled in as given and
    /// everything else left alone.
    fn event_query(start: Option<&str>, end: Option<&str>, show_past: bool) -> FinderQuery {
        FinderQuery {
            start: start.map(str::to_string),
            end: end.map(str::to_string),
            show_past: show_past.then(|| "on".to_string()),
            kind: vec![super::EVENT_KIND.to_string()],
            ..default_location_query(LAT, LNG)
        }
    }

    /// The UTC date `days` from today, as a date field submits it.
    fn utc_date_in(days: i64) -> String {
        let today = jiff::Timestamp::now()
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date();

        today
            .checked_add(jiff::Span::new().days(days))
            .unwrap()
            .to_string()
    }

    /// An event with a single instance, running from `begin` to `end` -- both
    /// SQLite modifiers relative to now -- and stored the way real instances
    /// are, with a `T` and a `Z` around the time of day.
    async fn add_event(pool: &SqlitePool, name: &str, begin: &str, end: &str) {
        sqlx::query("INSERT INTO events VALUES (?, ?, 0, NULL, NULL)")
            .bind(name)
            .bind(name)
            .execute(pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO event_instances VALUES (?, \
             strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?), \
             strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?))",
        )
        .bind(name)
        .bind(begin)
        .bind(end)
        .execute(pool)
        .await
        .unwrap();
    }

    /// A database holding an event that is over, one under way, and one still
    /// to come.
    async fn three_events() -> SqlitePool {
        let pool = database().await;
        add_event(&pool, "over", "-10 days", "-9 days").await;
        add_event(&pool, "under way", "-1 day", "+1 day").await;
        add_event(&pool, "to come", "+10 days", "+11 days").await;
        pool
    }

    /// The events a query keeps, by name.
    async fn matching(pool: &SqlitePool, query: &FinderQuery) -> Vec<String> {
        let mut qb = sqlx::QueryBuilder::new("SELECT ev.name FROM events ev WHERE 1 = 1");
        apply_event_filters(&mut qb, query);
        qb.push(" ORDER BY ev.name");

        qb.build_query_scalar::<String>()
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn an_event_under_way_has_not_passed() {
        let pool = three_events().await;

        assert_eq!(
            matching(&pool, &event_query(None, None, false)).await,
            vec!["to come".to_string(), "under way".to_string()]
        );
    }

    #[tokio::test]
    async fn past_events_come_back_when_they_are_asked_for() {
        let pool = three_events().await;

        assert_eq!(
            matching(&pool, &event_query(None, None, true)).await,
            vec![
                "over".to_string(),
                "to come".to_string(),
                "under way".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn a_date_range_includes_the_whole_of_its_last_day() {
        let pool = three_events().await;

        let last_day = utc_date_in(10);
        assert_eq!(
            matching(&pool, &event_query(None, Some(&last_day), false)).await,
            vec!["to come".to_string(), "under way".to_string()]
        );

        let day_before = utc_date_in(9);
        assert_eq!(
            matching(&pool, &event_query(None, Some(&day_before), false)).await,
            vec!["under way".to_string()]
        );
    }

    #[tokio::test]
    async fn a_start_date_reaches_back_past_the_present() {
        let pool = three_events().await;

        let a_fortnight_ago = utc_date_in(-14);
        assert_eq!(
            matching(&pool, &event_query(Some(&a_fortnight_ago), None, false)).await,
            vec![
                "over".to_string(),
                "to come".to_string(),
                "under way".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn a_date_that_is_not_one_is_no_date_at_all() {
        let pool = three_events().await;

        assert_eq!(
            matching(&pool, &event_query(Some("whenever"), Some(""), false)).await,
            vec!["to come".to_string(), "under way".to_string()]
        );
    }

    #[test]
    fn cleared_toggles_say_so_and_absent_ones_do_not() {
        let kinds = |kind: &[&str]| {
            SearchKinds::from_query(&FinderQuery {
                kind: kind.iter().map(|k| k.to_string()).collect(),
                ..default_location_query(LAT, LNG)
            })
        };

        // A link that predates the toggles, or a bare /finder.
        assert_eq!(kinds(&[]), SearchKinds::Both);
        assert_eq!(kinds(&["none", "project", "event"]), SearchKinds::Both);
        assert_eq!(kinds(&["none", "event"]), SearchKinds::EventsOnly);
        assert_eq!(kinds(&["none", "project"]), SearchKinds::ProjectsOnly);
        assert_eq!(kinds(&["none"]), SearchKinds::Neither);
    }

    /// The toggles ride on `kind[]`, and a browser percent-encodes those
    /// brackets on its way to the finder -- from the `/events` redirect, or
    /// from any link that was written out in full.
    #[tokio::test]
    async fn percent_encoded_kinds_still_parse() {
        use crate::form::QsQuery;
        use poem::FromRequest;

        let request = poem::Request::builder()
            .uri(poem::http::Uri::from_static("/finder?kind%5B%5D=event"))
            .finish();

        let QsQuery(query) = QsQuery::<FinderQuery>::from_request_without_body(&request)
            .await
            .unwrap();

        assert_eq!(SearchKinds::from_query(&query), SearchKinds::EventsOnly);
    }

    /// Everything a sidebar can be filled in with, so that what survives
    /// [`FinderQuery::as_offered`] is the whole of what a search is offered.
    fn every_filter() -> FinderQuery {
        FinderQuery {
            topic: vec!["Birds".to_string()],
            audience: vec!["Kids".to_string()],
            activity: vec!["At home".to_string()],
            misc_filter: vec!["has_app".to_string()],
            show_completed: Some("on".to_string()),
            start: Some("2026-10-01".to_string()),
            end: Some("2026-10-31".to_string()),
            show_past: Some("on".to_string()),
            ..default_location_query(LAT, LNG)
        }
    }

    #[test]
    fn a_search_of_both_kinds_is_offered_only_what_they_share() {
        let both = every_filter().as_offered(SearchKinds::Both, SingleKindFilters::HeldBack);

        // Shared ground: every kind of result can be judged by these.
        assert_eq!(both.topic, vec!["Birds".to_string()]);
        assert_eq!(both.lat, Some(LAT));

        // A project's filters would leave the events unvetted, and the dates
        // would leave the projects unvetted, so neither set is in play.
        assert!(both.audience.is_empty());
        assert!(both.activity.is_empty());
        assert!(both.misc_filter.is_empty());
        assert!(both.show_completed.is_none());
        assert!(both.start.is_none());
        assert!(both.end.is_none());
        assert!(both.show_past.is_none());
    }

    #[test]
    fn a_search_of_one_kind_is_offered_that_kinds_filters() {
        let projects =
            every_filter().as_offered(SearchKinds::ProjectsOnly, SingleKindFilters::HeldBack);

        assert_eq!(projects.audience, vec!["Kids".to_string()]);
        assert_eq!(projects.activity, vec!["At home".to_string()]);
        assert_eq!(projects.misc_filter, vec!["has_app".to_string()]);
        assert!(projects.show_completed.is_some());
        assert!(projects.start.is_none());
        assert!(projects.show_past.is_none());

        let events =
            every_filter().as_offered(SearchKinds::EventsOnly, SingleKindFilters::HeldBack);

        assert_eq!(events.start.as_deref(), Some("2026-10-01"));
        assert_eq!(events.end.as_deref(), Some("2026-10-31"));
        assert!(events.show_past.is_some());
        assert!(events.audience.is_empty());
        assert!(events.show_completed.is_none());
    }

    /// Marking the filters instead keeps them in force wherever the sidebar
    /// still shows them, which is the whole point of the other policy: what is
    /// on display is what narrowed the results, either way round.
    #[test]
    fn marked_filters_are_in_force_in_a_search_of_both_kinds() {
        let both = every_filter().as_offered(SearchKinds::Both, SingleKindFilters::Marked);

        assert_eq!(both.audience, vec!["Kids".to_string()]);
        assert_eq!(both.start.as_deref(), Some("2026-10-01"));
        assert!(both.show_completed.is_some());
        assert!(both.show_past.is_some());

        // A kind the search does not cover still has nothing to say about it.
        let events = every_filter().as_offered(SearchKinds::EventsOnly, SingleKindFilters::Marked);

        assert!(events.audience.is_empty());
        assert_eq!(events.end.as_deref(), Some("2026-10-31"));
    }

    /// Only one policy can be on the page at a time, and the stylesheet knows
    /// which by the class the form carries.
    #[test]
    fn each_policy_marks_the_form_its_own_way() {
        assert!(SingleKindFilters::HeldBack.form_class().is_empty());
        assert!(!SingleKindFilters::Marked.form_class().is_empty());
    }

    /// What is out of scope stays in the link, or narrowing the search back to
    /// its kind would find the field empty.
    #[test]
    fn a_link_still_carries_a_filter_the_search_set_aside() {
        let query = FinderQuery {
            kind: vec![PROJECT_KIND.to_string(), EVENT_KIND.to_string()],
            ..every_filter()
        };

        assert!(
            query
                .as_offered(SearchKinds::Both, SingleKindFilters::HeldBack)
                .audience
                .is_empty()
        );
        assert!(query.to_query_string().contains("audience[]=Kids"));
    }

    #[test]
    fn a_link_carries_the_filters_that_narrow_the_search() {
        let query = FinderQuery {
            kind: vec!["none".to_string(), "event".to_string()],
            start: Some("2026-10-01".to_string()),
            end: Some("whenever".to_string()),
            show_past: Some("on".to_string()),
            ..default_location_query(LAT, LNG)
        };

        let params = query.to_query_string();

        assert!(params.contains("kind[]=event"), "{params}");
        assert!(!params.contains("kind[]=project"), "{params}");
        assert!(params.contains("start=2026-10-01"), "{params}");
        assert!(params.contains("show_past=on"), "{params}");
        // A date field that holds no date says nothing.
        assert!(!params.contains("end="), "{params}");

        // Both kinds is what a link with no kinds at all already means.
        let both = FinderQuery {
            kind: vec![
                "none".to_string(),
                "project".to_string(),
                "event".to_string(),
            ],
            ..default_location_query(LAT, LNG)
        };
        assert!(!both.to_query_string().contains("kind[]"));
    }

    #[tokio::test]
    async fn a_region_split_at_the_antimeridian_is_measured() {
        let pool = database().await;
        add(&pool, LocatedTable::Projects, "split", Some(SPLIT), None).await;

        let rows = ranked(&pool, LocatedTable::Projects).await;
        let (_, distance, comparison) = row(&rows, "split");

        // The searcher stands inside it, and its two 10x10 degree parts at
        // these latitudes come to about two million square kilometres.
        assert_eq!(*distance, Some(0.0));
        assert!((1_400.0..1_430.0).contains(comparison), "{comparison}");
    }

    #[tokio::test]
    async fn an_unmeasurable_footprint_ranks_by_its_point() {
        let pool = database().await;

        for (name, regions) in [("long way", LONG_WAY), ("unwrapped", UNWRAPPED)] {
            add(
                &pool,
                LocatedTable::Projects,
                name,
                Some(regions),
                Some(POINT_NORTH),
            )
            .await;
        }

        let rows = ranked(&pool, LocatedTable::Projects).await;

        for name in ["long way", "unwrapped"] {
            let (_, distance, comparison) = row(&rows, name);
            let distance = distance.unwrap();

            assert!((110.0..112.0).contains(&distance), "{name}: {distance}");
            assert_eq!(*comparison, distance, "{name}");
        }
    }

    #[tokio::test]
    async fn an_unmeasurable_footprint_without_a_point_covers_the_world() {
        let pool = database().await;
        add(&pool, LocatedTable::Projects, "world", Some(WORLD), None).await;
        add(
            &pool,
            LocatedTable::Projects,
            "long way",
            Some(LONG_WAY),
            None,
        )
        .await;
        add(
            &pool,
            LocatedTable::Events,
            "long way",
            Some(LONG_WAY),
            None,
        )
        .await;

        for table in [LocatedTable::Projects, LocatedTable::Events] {
            for (name, distance, comparison) in ranked(&pool, table).await {
                assert_eq!(distance, None, "{table:?} {name}");
                assert_eq!(comparison, WHOLE_WORLD_KM, "{table:?} {name}");
            }
        }
    }

    #[test]
    fn delta_description_renders_as_plain_text() {
        let delta = r#"{"ops":[{"insert":"Fight malaria from home by analyzing images.\n"}]}"#;
        let snippet = make_snippet(delta);
        assert_eq!(snippet, "Fight malaria from home by analyzing images.");
        assert!(!snippet.contains('{'));
    }

    #[test]
    fn html_description_is_stripped_to_text() {
        let snippet = make_snippet("<p>Hello <strong>world</strong></p>");
        assert!(snippet.contains("Hello"));
        assert!(snippet.contains("world"));
        assert!(!snippet.contains('<'));
    }

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(make_snippet("Just plain text"), "Just plain text");
    }

    /// One project per case the finder's listing rule decides, named for it.
    async fn finder_projects() -> SqlitePool {
        // SAFETY: mod_spatialite is a trusted extension, the same one the
        // application loads.
        let options = unsafe { SqliteConnectOptions::new().extension("mod_spatialite") };
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.filename(":memory:"))
            .await
            .unwrap();

        sqlx::query(
            "CREATE TABLE projects (project_id TEXT, name TEXT, flagged INTEGER, \
             duplicate INTEGER, is_test INTEGER, draft INTEGER, status TEXT, \"end\" TEXT, \
             point BLOB)",
        )
        .execute(&pool)
        .await
        .unwrap();

        for (name, status, end, flagged, duplicate, is_test, draft) in [
            ("pending", "Pending", "", 0, 0, 0, 0),
            ("active", "Active", "", 0, 0, 0, 0),
            ("periodic", "Periodically Active", "", 0, 0, 0, 0),
            ("hiatus", "Hiatus", "", 0, 0, 0, 0),
            ("complete", "Complete", "", 0, 0, 0, 0),
            ("expired", "Expired", "2000-01-01", 0, 0, 0, 0),
            ("hidden", "Hidden", "", 0, 0, 0, 0),
            ("ended", "Active", "2000-01-01", 0, 0, 0, 0),
            ("flagged", "Active", "", 1, 0, 0, 0),
            ("duplicate", "Active", "", 0, 1, 0, 0),
            ("test", "Active", "", 0, 0, 1, 0),
            ("stale-draft", "Active", "", 0, 0, 0, 1),
        ] {
            sqlx::query("INSERT INTO projects VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)")
                .bind(name)
                .bind(name)
                .bind(flagged)
                .bind(duplicate)
                .bind(is_test)
                .bind(draft)
                .bind(status)
                .bind(end)
                .execute(&pool)
                .await
                .unwrap();
        }

        pool
    }

    /// The projects a finder search lists, by name.
    async fn finder_lists(pool: &SqlitePool, show_completed: bool) -> Vec<String> {
        let query = FinderQuery {
            show_completed: show_completed.then(|| "on".to_string()),
            ..default_location_query(LAT, LNG)
        };

        let mut qb = sqlx::QueryBuilder::new("SELECT p.name FROM projects p WHERE 1 = 1");
        super::apply_project_filters(&mut qb, &query);
        qb.push(" ORDER BY p.name");

        qb.build_query_scalar::<String>()
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn the_finder_lists_what_can_still_be_joined() {
        let pool = finder_projects().await;

        assert_eq!(
            finder_lists(&pool, false).await,
            ["active", "hiatus", "pending", "periodic", "stale-draft"]
        );
    }

    #[tokio::test]
    async fn finished_projects_come_back_when_asked_for_but_hidden_ones_never_do() {
        let pool = finder_projects().await;

        assert_eq!(
            finder_lists(&pool, true).await,
            [
                "active",
                "complete",
                "ended",
                "expired",
                "hiatus",
                "pending",
                "periodic",
                "stale-draft"
            ]
        );
    }
}
