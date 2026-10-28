use anyhow::Error;
use common::{
    message::log::{ProjectLikesResponse, RatingSetRequest},
    quill::content_to_html,
    service::request,
    shared::db,
    timestring::TimeString,
};
use poem::{
    IntoResponse, handler,
    session::Session,
    web::{Html, Path, Query},
};
use sailfish::TemplateOnce;
use std::sync::OnceLock;
use ulid::Ulid;

use crate::local_time::{TimeFormat, local_time};
use crate::middleware::analytics::{Device, Interaction};
use crate::middleware::authorization::Authenticated;
use crate::page_change::last_changes;
use crate::peering::LoadStorePeered;

use super::tracking::{self, TrackedAction, TrackedPage};
use super::{
    Exchange, ExchangeInfo, HiddenProject, SearchParams, TemplateExchange, TemplateUser, access,
};
use crate::component::html_escape;
use crate::middleware::error_pages::error_response;
use crate::project::all::ProjectAll;
use crate::project::location::Where;
use crate::project::{ProjectStatus, push_listing_rule};

// Cache for filter options (loaded once on first use)
static FILTER_CACHE: OnceLock<FilterOptions> = OnceLock::new();

#[derive(Clone)]
struct FilterOptions {
    activities: Vec<(String, String)>,
    topics: Vec<(String, String)>,
    audiences: Vec<(String, String)>,
}

async fn load_filter_options() -> Result<FilterOptions, Error> {
    let activity_rows =
        sqlx::query!("SELECT DISTINCT activity FROM project_activities ORDER BY activity")
            .fetch_all(common::shared::db())
            .await?;
    let activities: Vec<(String, String)> = activity_rows
        .into_iter()
        .map(|r| (r.activity.clone(), r.activity))
        .collect();

    let topic_rows = sqlx::query!("SELECT DISTINCT topic FROM project_topics ORDER BY topic")
        .fetch_all(common::shared::db())
        .await?;
    let topics: Vec<(String, String)> = topic_rows
        .into_iter()
        .map(|r| (r.topic.clone(), r.topic))
        .collect();

    let audience_rows =
        sqlx::query!("SELECT DISTINCT audience FROM project_audiences ORDER BY audience")
            .fetch_all(common::shared::db())
            .await?;
    let audiences: Vec<(String, String)> = audience_rows
        .into_iter()
        .map(|r| (r.audience.clone(), r.audience))
        .collect();

    Ok(FilterOptions {
        activities,
        topics,
        audiences,
    })
}

async fn get_filter_options() -> Result<FilterOptions, Error> {
    // Try to get from cache
    if let Some(cached) = FILTER_CACHE.get() {
        return Ok(cached.clone());
    }

    // Not cached yet, load from database
    let options = load_filter_options().await?;

    // Try to store in cache (ignore if another thread already did)
    let _ = FILTER_CACHE.set(options.clone());

    Ok(options)
}

// Helper functions

/// Build base query with common WHERE clause (with or without FTS)
/// Exclude the projects this exchange has hidden, unless the viewer runs it.
///
/// An exchange's staff are shown their own hidden projects so that moderation
/// decisions stay visible to the people who made them; everybody else sees
/// the exchange as it is meant to read.
fn push_hidden_exclusion(
    query_builder: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    exchange_id: Ulid,
    scope: &SearchScope,
) {
    if scope.include_hidden {
        return;
    }

    query_builder.push(
        r#" AND p.project_id NOT IN (
                SELECT project_id FROM exchange_hidden_projects
                WHERE exchange_id = "#,
    );
    query_builder.push_bind(exchange_id.to_string());
    query_builder.push(")");
}

/// What a search is actually asking for, once its phrases have been read.
///
/// Region tick-boxes arrive as phrases carrying a region's name, so the
/// phrase list has to be split before any of it becomes SQL: the names
/// become a region filter, and only what is left is text to match. A phrase
/// naming no region stays text, which is what a typed search term is.
///
/// `include_hidden` carries the viewer's standing, since an exchange's own
/// staff are shown the projects they have hidden from everybody else.
struct SearchScope {
    regions: Vec<i64>,
    phrases: Vec<String>,
    include_hidden: bool,

    /// Stamped once per search so every query in it -- results and count
    /// alike -- draws the line between running and finished at the same
    /// instant.
    now: String,
}

impl SearchScope {
    async fn resolve(params: &SearchParams, include_hidden: bool) -> Result<Self, Error> {
        let named = crate::region::ids_for_names(&params.phrase).await?;

        let phrases = params
            .phrase
            .iter()
            .filter(|phrase| !phrase.trim().is_empty())
            .filter(|phrase| !named.iter().any(|(_, name)| name == *phrase))
            .cloned()
            .collect();

        Ok(SearchScope {
            regions: named.into_iter().map(|(id, _)| id).collect(),
            phrases,
            include_hidden,
            now: TimeString::now().to_string(),
        })
    }

    fn has_text_search(&self) -> bool {
        !self.phrases.is_empty()
    }
}

fn build_base_where_clause<'a>(
    query_builder: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    exchange_id: Ulid,
    scope: &'a SearchScope,
    for_count: bool,
) {
    let has_text_search = scope.has_text_search();

    if has_text_search {
        // Use FTS5 for text search
        if for_count {
            query_builder.push("SELECT COUNT(*) FROM projects p");
        } else {
            query_builder.push(
                r#"SELECT
                    p.project_id,
                    ent.slug,
                    p.name,
                    p.image,
                    p.image_credit,
                    p.goal,
                    p.task,
                    COALESCE(pex.name, ''),
                    'https://scistarter.org/-admin/projects/' || p.project_id
                FROM projects p"#,
            );
        }

        query_builder.push(
            r#"
            JOIN projects_fts f ON p.project_id = f.project_id
            LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
            LEFT JOIN exchanges pex ON pex.exchange_id = p.exchange
            WHERE projects_fts MATCH "#,
        );

        // Combine phrases with OR for FTS5
        let search_query = scope
            .phrases
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(" OR ");
        query_builder.push_bind(search_query);

        query_builder.push(
            r#"
            AND ent.publish = 1
            "#,
        );

        push_hidden_exclusion(query_builder, exchange_id, scope);
    } else {
        // No text search, use regular query
        if for_count {
            query_builder.push("SELECT COUNT(*) FROM projects p");
        } else {
            query_builder.push(
                r#"SELECT
                    p.project_id,
                    ent.slug,
                    p.name,
                    p.image,
                    p.image_credit,
                    p.goal,
                    p.task,
                    COALESCE(pex.name, ''),
                    'https://scistarter.org/-admin/projects/' || p.project_id
                FROM projects p"#,
            );
        }

        query_builder.push(
            r#"
            LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
            LEFT JOIN exchanges pex ON pex.exchange_id = p.exchange
            WHERE ent.publish = 1
            "#,
        );

        push_hidden_exclusion(query_builder, exchange_id, scope);
    }
}

/// The `location_type` a project that can be done anywhere carries.
///
/// Spelled out rather than coded, because that is how orrery's sync sends it
/// and therefore how hydra stores it.
const GLOBAL_LOCATION_TYPE: &str = crate::project::location::ANYWHERE;

/// Narrow a query to the projects this exchange is a view of.
///
/// This is orrery's `Exchange.refresh_project_list`, applied live rather
/// than materialized nightly, and the two must move together:
///
/// - An exchange is a view of the whole catalogue, unless it excludes
///   SciStarter. Then it lists only the projects submitted through it, plus
///   those carrying one of its include-tags or include-origins -- levers
///   that do nothing otherwise, which is why SciStarter Australia's
///   `australia` include-tag has never had any effect.
/// - Excluded origins come off whatever that let in, its own projects too.
/// - A search regex, if set, must match the project's name or its tags,
///   the tags written the way orrery stores them: `||one||two||`.
/// - A region keeps the projects that reach it plus, when `include_global`
///   (the finder's "include global projects" box), the ones that can be
///   done anywhere, which is how "available in Australia" is expressed.
///   Orrery's `Exchange::find` narrows the same way. The region test reads
///   the cached matches rather than measuring polygons per query, so it
///   agrees with the region tick-boxes by construction.
fn push_membership(
    query_builder: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    exchange: &ExchangeInfo,
    include_global: bool,
) {
    let settings = &exchange.settings;

    if settings.exclude_scistarter {
        query_builder.push(" AND (p.exchange = ");
        query_builder.push_bind(exchange.exchange_id.to_string());

        if !settings.include_tags.is_empty() {
            query_builder.push(
                " OR p.project_id IN (SELECT project_id FROM project_tags WHERE upper(tag) IN (",
            );
            let mut separated = query_builder.separated(", ");
            for tag in settings.include_tags.iter() {
                separated.push("upper(");
                separated.push_bind_unseparated(tag.clone());
                separated.push_unseparated(")");
            }
            query_builder.push("))");
        }

        if !settings.include_origins.is_empty() {
            query_builder.push(" OR p.origin IN (");
            let mut separated = query_builder.separated(", ");
            for origin in settings.include_origins.iter() {
                separated.push_bind(origin.clone());
            }
            query_builder.push(")");
        }

        query_builder.push(")");
    }

    if !settings.excluded_origins.is_empty() {
        query_builder.push(" AND p.origin NOT IN (");

        let mut separated = query_builder.separated(", ");
        for origin in settings.excluded_origins.iter() {
            separated.push_bind(origin.clone());
        }

        query_builder.push(")");
    }

    if !settings.search.is_empty() {
        let pattern = search_pattern(&settings.search);

        query_builder.push(" AND (p.name REGEXP ");
        query_builder.push_bind(pattern.clone());
        query_builder.push(
            " OR (SELECT '||' || group_concat(tag, '||') || '||' FROM project_tags t \
             WHERE t.project_id = p.project_id) REGEXP ",
        );
        query_builder.push_bind(pattern);
        query_builder.push(")");
    }

    if let Some(region_id) = settings.region_id {
        query_builder.push(
            " AND (p.project_id IN (SELECT project_id FROM project_regions WHERE region_id = ",
        );
        query_builder.push_bind(region_id);
        query_builder.push(")");
        if include_global {
            query_builder.push(" OR p.location_type = ");
            query_builder.push_bind(GLOBAL_LOCATION_TYPE);
        }
        query_builder.push(")");
    }
}

/// An exchange's search setting as a pattern for SQLite's `REGEXP`.
///
/// Orrery matches it with Postgres's `~*`, so it is matched here without
/// regard to case as well.
pub fn search_pattern(search: &str) -> String {
    format!("(?i){search}")
}

/// Narrow a search to the projects it may list.
///
/// A project flagged, marked a duplicate or marked a test is never listed,
/// and neither is one whose status is not in
/// [`ProjectStatus::LISTED_WHEN_INACTIVE`]. Unless inactive projects were
/// asked for, the search is further narrowed to Active projects that have
/// not passed their end date -- "inactive" covering both, since they are
/// different ways for a project to have finished.
///
/// Orrery's exchange search applies the same rule (`push_listing_filter` in
/// src/common.rs), and the two must move together so an exchange lists the
/// same projects on either side of the cutover.
fn push_listing_filter(
    query_builder: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    params: &SearchParams,
    now: &str,
) {
    if params.inactive {
        push_listing_rule(query_builder, &ProjectStatus::LISTED_WHEN_INACTIVE);
    } else {
        push_listing_rule(query_builder, &[ProjectStatus::Active]);
        query_builder.push(" AND (p.\"end\" IS NULL OR p.\"end\" = '' OR p.\"end\" > ");
        query_builder.push_bind(now.to_owned());
        query_builder.push(")");
    }
}

/// Apply common search filters to a query builder
fn apply_search_filters<'a>(
    query_builder: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    exchange: &'a ExchangeInfo,
    params: &'a SearchParams,
    scope: &'a SearchScope,
) {
    push_membership(query_builder, exchange, params.global);
    push_listing_filter(query_builder, params, &scope.now);

    // Several regions read as "any of these", the way several tick-boxes do.
    if !scope.regions.is_empty() {
        query_builder.push(
            " AND p.project_id IN (SELECT project_id FROM project_regions WHERE region_id IN (",
        );

        let mut separated = query_builder.separated(", ");
        for region_id in scope.regions.iter() {
            separated.push_bind(*region_id);
        }

        query_builder.push("))");
    }

    // Add activity filter
    if let Some(ref activity) = params.activity {
        if !activity.is_empty() {
            query_builder.push(
                " AND p.project_id IN (SELECT project_id FROM project_activities WHERE activity = ",
            );
            query_builder.push_bind(activity);
            query_builder.push(")");
        }
    }

    // Add topic filter
    if let Some(ref topic) = params.topic {
        if !topic.is_empty() {
            query_builder
                .push(" AND p.project_id IN (SELECT project_id FROM project_topics WHERE topic = ");
            query_builder.push_bind(topic);
            query_builder.push(")");
        }
    }

    // Add audience filters
    if !params.audience.is_empty() {
        query_builder.push(
            " AND p.project_id IN (SELECT project_id FROM project_audiences WHERE audience IN (",
        );
        let mut separated = query_builder.separated(", ");
        for audience in &params.audience {
            separated.push_bind(audience);
        }
        query_builder.push("))");
    }
}

/// Create a TemplateExchange with proper authorization checking
async fn create_template_exchange(
    exchange_info: ExchangeInfo,
    auth: &Authenticated,
) -> Result<TemplateExchange, Error> {
    let user_is_authorized =
        access::may_manage(exchange_info.exchange_id, auth.0.as_deref()).await?;

    Ok(TemplateExchange {
        info: exchange_info,
        user_is_authorized,
    })
}

// Data structures for templates

#[derive(Debug, Clone)]
pub struct ProjectSummary {
    pub project_id: String,
    pub slug: String,
    pub name: String,
    pub image: String,
    pub image_credit: String,
    pub goal: String,
    pub task: String,

    /// Filled in by [`ProjectSummary::attach_locations`], which the search
    /// handlers call once per page of results.
    pub location: Where,
}

impl ProjectSummary {
    /// Fill in `location` for a page of results in one query.
    ///
    /// A card shows a project's location as text, never as a map, so this
    /// reads the columns the wording needs and whether a polygon exists --
    /// not the polygon itself, which on a page of twenty cards would be
    /// megabytes nobody looks at.
    async fn attach_locations(projects: &mut [ProjectSummary]) -> Result<(), Error> {
        if projects.is_empty() {
            return Ok(());
        }

        let mut query_builder = sqlx::QueryBuilder::new(
            r#"select
                 "project_id",
                 "location_type",
                 "location_landuse",
                 "location_description",
                 ("regions" is not null),
                 "city",
                 "state",
                 "country",
                 "address"
               from projects
               where "project_id" in ("#,
        );

        let mut separated = query_builder.separated(", ");
        for project in projects.iter() {
            separated.push_bind(project.project_id.clone());
        }
        query_builder.push(")");

        let rows = query_builder
            .build_query_as::<(
                String,
                String,
                String,
                String,
                bool,
                String,
                String,
                String,
                String,
            )>()
            .fetch_all(common::shared::db())
            .await?;

        let locations: std::collections::HashMap<String, Where> = rows
            .into_iter()
            .map(
                |(
                    project_id,
                    location_type,
                    landuse,
                    description,
                    coverage,
                    city,
                    state,
                    country,
                    address,
                )| {
                    (
                        project_id,
                        Where::new(
                            &location_type,
                            &landuse,
                            &description,
                            coverage,
                            &city,
                            &state,
                            &country,
                            &address,
                        ),
                    )
                },
            )
            .collect();

        for project in projects.iter_mut() {
            if let Some(location) = locations.get(&project.project_id) {
                project.location = location.clone();
            }
        }

        Ok(())
    }
}

/// Full project detail for exchange project pages
#[derive(Debug, Clone)]
pub struct ExchangeProjectDetail {
    pub id: String, // slug for URL purposes
    pub slug: String,
    pub name: String,
    pub image: String,
    pub image_credit: String,
    pub goal: String,
    pub task: String,
    pub description: String,
    pub how_to_join: String,
    pub presenter: String,
    pub url: Option<String>,
    pub twitter_name: Option<String>,
    pub facebook_page: Option<String>,
    pub total_expense: Option<f64>,
    pub indoors: bool,
    pub outdoors: bool,
    pub tags: Vec<String>,
    pub updated: TimeString,

    // Lists of (id, name) tuples
    pub activities: Vec<(String, String)>,
    pub topics: Vec<(String, String)>,
    pub audience: Vec<(String, String)>,

    pub location: Where,

    /// The coverage polygon as GeoJSON, present only when `location` reports
    /// a map to draw.
    pub coverage: Option<String>,

    // User-specific data
    pub user_is_authorized: bool,
    pub liked: bool,
    pub likes: i64,
    pub saved: bool,
    pub joined: bool,

    pub reviews: Vec<ReviewForDisplay>,
}

/// Reviews are enriched for display by the detail page's shared helper, so
/// exchange pages render the same struct it produces.
pub use crate::component::detail_page::ReviewForDisplay;

#[derive(Debug, Clone)]
pub struct Pagination {
    pub page: usize,
    pub page_size: usize,
}

impl SearchParams {
    pub fn query_string(&self) -> String {
        let mut parts = Vec::new();

        for phrase in &self.phrase {
            if !phrase.is_empty() {
                parts.push(format!("phrase[]={}", urlencoding::encode(phrase)));
            }
        }

        if let Some(activity) = &self.activity {
            if !activity.is_empty() {
                parts.push(format!("activity={}", urlencoding::encode(activity)));
            }
        }

        if let Some(topic) = &self.topic {
            if !topic.is_empty() {
                parts.push(format!("topic={}", urlencoding::encode(topic)));
            }
        }

        for audience in &self.audience {
            parts.push(format!("audience[]={}", urlencoding::encode(audience)));
        }

        if self.inactive {
            parts.push("inactive=true".to_string());
        }

        parts.join("&")
    }
}

// Template structs

#[derive(TemplateOnce)]
#[template(path = "exchange/navbar.stpl.html")]
struct NavigationBar<'a> {
    exchange: &'a TemplateExchange,
    user: &'a TemplateUser,
}

#[derive(TemplateOnce)]
#[template(path = "exchange/projectcard.stpl.html")]
struct ProjectCard<'a> {
    exchange: &'a TemplateExchange,
    project: &'a ProjectSummary,
}

#[derive(TemplateOnce)]
#[template(path = "exchange/pageselector.stpl.html")]
struct PageSelector<'a> {
    exchange: &'a TemplateExchange,
    params: &'a SearchParams,
    page: usize,
    page_size: usize,
    total: usize,
}

impl<'a> PageSelector<'a> {
    fn last_page(&self) -> usize {
        ((self.total + self.page_size - 1) / self.page_size).max(1)
    }
}

#[derive(TemplateOnce)]
#[template(path = "exchange/searchresults.stpl.html")]
struct SearchResults<'a> {
    exchange: &'a TemplateExchange,
    projects: &'a [ProjectSummary],
    pagination: &'a Pagination,
    params: &'a SearchParams,
    total: usize,
}

#[derive(TemplateOnce)]
#[template(path = "exchange/index.stpl.html")]
struct ExchangeIndex<'a> {
    exchange: &'a TemplateExchange,
    user: &'a TemplateUser,
    csrftoken: String,
    predefined: Vec<(String, String)>,
    activities: Vec<(String, String)>,
    topics: Vec<(String, String)>,
    audiences: Vec<(String, String)>,
    projects: &'a [ProjectSummary],
    pagination: &'a Pagination,
    params: &'a SearchParams,
    total: usize,
}

#[derive(TemplateOnce)]
#[template(path = "exchange/project.stpl.html")]
struct ExchangeProjectPage {
    exchange: TemplateExchange,
    user: TemplateUser,
    project: ExchangeProjectDetail,
    csrftoken: String,

    /// The squelch form, rendered, for the exchange's own people only.
    squelch: Option<String>,
}

impl ExchangeProjectPage {
    fn encode(&self, s: &str) -> String {
        urlencoding::encode(s).to_string()
    }
}

/// An event as an exchange's page for it shows it.
pub struct ExchangeEventDetail {
    pub id: Ulid,
    pub slug: String,
    pub name: String,
    pub presenter: String,
    pub image: String,
    pub image_credit: String,
    pub url: Option<String>,
    /// Rendered, sanitized HTML.
    pub description: String,
    /// Rendered, sanitized HTML.
    pub how_to_join: String,
    pub event_type: String,
    /// When it happens, earliest first: each a begin and an end.
    pub instances: Vec<(String, String)>,
    /// The venue, when the event names one.
    pub location_name: String,
    pub location: Where,
    /// The event's area as GeoJSON, present only when `location` reports a
    /// map to draw.
    pub coverage: Option<String>,
    pub tags: Vec<String>,
}

#[derive(TemplateOnce)]
#[template(path = "exchange/event.stpl.html")]
struct ExchangeEventPage {
    exchange: TemplateExchange,
    user: TemplateUser,
    event: ExchangeEventDetail,
    csrftoken: String,
}

impl ExchangeEventPage {
    fn encode(&self, s: &str) -> String {
        urlencoding::encode(s).to_string()
    }
}

async fn load_exchange_event_detail(id: Ulid, slug: &str) -> Result<ExchangeEventDetail, Error> {
    let id_str = id.to_string();

    let row = sqlx::query!(
        r#"
        SELECT name, presenter, image, image_credit, url, description, how_to_join,
               event_type as "event_type: crate::event::EventType",
               location_type, entirely_online as "entirely_online: bool",
               location_name, location_description, address, city, state, country,
               AsGeoJSON(regions) as "coverage: Option<String>"
        FROM events
        WHERE event_id = ?
        "#,
        id_str
    )
    .fetch_one(db())
    .await?;

    let instances = sqlx::query!(
        "SELECT begin, end FROM event_instances WHERE event_id = ? ORDER BY begin",
        id_str
    )
    .fetch_all(db())
    .await?
    .into_iter()
    .map(|row| (row.begin, row.end))
    .collect();

    let tags = sqlx::query_scalar!(
        "SELECT tag FROM event_tags WHERE event_id = ? ORDER BY tag",
        id_str
    )
    .fetch_all(db())
    .await?
    .into_iter()
    .filter(|tag| !crate::project::is_hidden_tag(tag))
    .collect();

    let coverage = row.coverage.flatten();
    let location = if row.entirely_online {
        Where::Online
    } else {
        Where::new(
            &row.location_type,
            "",
            &row.location_description,
            coverage.is_some(),
            &row.city,
            &row.state,
            &row.country,
            &row.address,
        )
    };
    let coverage = if location.has_map() { coverage } else { None };

    Ok(ExchangeEventDetail {
        id,
        slug: slug.to_string(),
        name: row.name,
        presenter: row.presenter,
        image: row.image,
        image_credit: row.image_credit,
        url: (!row.url.is_empty()).then_some(row.url),
        description: content_to_html(&row.description)
            .map(|(html, _)| html)
            .unwrap_or(row.description),
        how_to_join: content_to_html(&row.how_to_join)
            .map(|(html, _)| html)
            .unwrap_or(row.how_to_join),
        event_type: row.event_type.to_string(),
        instances,
        location_name: row.location_name,
        location,
        coverage,
        tags,
    })
}

/// An exchange's page for one event.
///
/// An unpublished event -- one waiting in the approval queue -- is shown
/// only to the exchange's own people, as an unpublished project is.
#[handler]
pub async fn exchange_event(
    Path((path_id, slug)): Path<(i64, String)>,
    auth: Authenticated,
    device: Device,
    interaction: Interaction,
    session: &Session,
) -> Result<poem::Response, Error> {
    let exchange_info = ExchangeInfo::load_by_id(path_id).await?;
    let exchange = create_template_exchange(exchange_info.clone(), &auth).await?;

    let row = sqlx::query!(
        r#"
        SELECT e.event_id, ent.publish as "publish!: bool"
        FROM events e
        JOIN entities ent ON ent.target = e.event_id AND ent.kind = 'Event'
        WHERE ent.slug = ?
        "#,
        slug
    )
    .fetch_optional(db())
    .await?
    .ok_or_else(|| anyhow::anyhow!("Event not found"))?;

    if !row.publish && !exchange.user_is_authorized {
        return Err(anyhow::anyhow!("Event not found"));
    }

    let event_id = Ulid::from_string(&row.event_id)?;

    tracking::record(
        exchange_info.exchange_id,
        auth.0.as_ref().map(|user| user.user_id),
        &device,
        &interaction,
        TrackedPage::Event,
        Some(event_id),
        TrackedAction::View,
        None,
    )
    .await?;

    let event = load_exchange_event_detail(event_id, &slug).await?;

    let csrftoken = session
        .get::<String>("csrf-token")
        .unwrap_or_else(|| "".to_string());

    Ok(Html(
        ExchangeEventPage {
            exchange,
            user: TemplateUser::from(auth),
            event,
            csrftoken,
        }
        .render_once()?,
    )
    .into_response())
}

/// The slug of the project an orrery-era exchange address names, if any.
///
/// Orrery addressed an exchange's projects by their numeric entity id
/// (`/exchange/38/1214/project.html`), which hydra keeps as the project's
/// `legacy_v2_project_id`; hydra addresses them by slug. Pages taking a
/// project from the path try it as a slug first, since nothing stops a
/// slug being all digits, and fall back to this so that links made before
/// the cutover keep working, redirected to where the project lives now.
async fn orrery_project_slug(
    pool: &sqlx::SqlitePool,
    segment: &str,
) -> Result<Option<String>, Error> {
    let Ok(legacy_id) = segment.parse::<i64>() else {
        return Ok(None);
    };

    Ok(sqlx::query_scalar!(
        r#"
        SELECT ent.slug as "slug!"
        FROM projects p
        JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
        WHERE p.legacy_v2_project_id = ?
        "#,
        legacy_id
    )
    .fetch_optional(pool)
    .await?)
}

/// Follow a project or event out to its own website, from an exchange.
///
/// Recorded as a referral -- the "visit" the exchange's data page counts.
/// Someone signed in who follows a project out is counted as having joined
/// it, as orrery counts them. A project's sign-up address is preferred to
/// its website. The redirect is temporary, so a browser asks again, and is
/// counted again, every time.
#[handler]
pub async fn visit(
    Path((path_id, slug)): Path<(i64, String)>,
    auth: Authenticated,
    device: Device,
    interaction: Interaction,
) -> Result<poem::Response, Error> {
    let exchange = ExchangeInfo::load_by_id(path_id).await?;

    let target = sqlx::query!(
        r#"
        SELECT ent.kind as "kind: crate::entity::EntityKind", ent.target as "target!",
               CASE ent.kind
                 WHEN 'Project' THEN (SELECT CASE WHEN p.signup_url != '' THEN p.signup_url ELSE p.url END
                                      FROM projects p WHERE p.project_id = ent.target)
                 WHEN 'Event' THEN (SELECT ev.url FROM events ev WHERE ev.event_id = ent.target)
               END as "url: String"
        FROM entities ent
        WHERE ent.slug = ? AND ent.kind IN ('Project', 'Event')
        "#,
        slug
    )
    .fetch_optional(db())
    .await?;

    let Some(target) = target else {
        return Ok(match orrery_project_slug(db(), &slug).await? {
            Some(slug) => {
                poem::web::Redirect::permanent(format!("/exchange/{path_id}/{slug}/visit"))
                    .into_response()
            }
            None => error_response(poem::http::StatusCode::NOT_FOUND, ""),
        });
    };

    let id = Ulid::from_string(&target.target)?;
    let page = match target.kind {
        crate::entity::EntityKind::Event => TrackedPage::Event,
        _ => TrackedPage::Project,
    };
    let user_id = auth.0.as_ref().map(|user| user.user_id);
    let url = target.url.filter(|url| !url.trim().is_empty());

    let Some(url) = url else {
        tracking::record(
            exchange.exchange_id,
            user_id,
            &device,
            &interaction,
            page,
            Some(id),
            TrackedAction::VisitMissing,
            None,
        )
        .await?;

        return Ok(error_response(
            poem::http::StatusCode::NOT_FOUND,
            "This has not provided a website for you to visit.",
        ));
    };

    let url = url.trim();
    let url = if url.starts_with("http") {
        url.to_string()
    } else {
        format!("http://{url}")
    };

    if let (TrackedPage::Project, Some(user_id)) = (page, user_id) {
        use common::message::user::{ProjectJoinRequest, ProjectJoinResponse};

        let _: ProjectJoinResponse = request(
            "user.joins.create",
            ProjectJoinRequest {
                user_id,
                project_id: id.to_string(),
            },
        )
        .await?;
    }

    tracking::record(
        exchange.exchange_id,
        user_id,
        &device,
        &interaction,
        page,
        Some(id),
        TrackedAction::Visit,
        Some(url.clone()),
    )
    .await?;

    Ok(poem::web::Redirect::temporary(url).into_response())
}

#[derive(TemplateOnce)]
#[template(path = "exchange/likebutton.stpl.html")]
struct LikeButton {
    liked: bool,
    likes: i64,
    exchange_id: i64,
    project_id: String,
    user_id: Option<Ulid>,
}

#[derive(TemplateOnce)]
#[template(path = "exchange/savebutton.stpl.html")]
struct SaveButton {
    id: String,
    saved: bool,
    exchange_id: i64,
    project_id: String,
}

#[derive(TemplateOnce)]
#[template(path = "exchange/reviewentry.stpl.html")]
struct ReviewEntry<'a> {
    review: &'a ReviewForDisplay,
    oob: bool,
}

/// Load full project detail for exchange project page
async fn load_exchange_project_detail(
    project_id: &str,
    project_slug: &str,
    user_info: Option<&std::sync::Arc<common::message::user::BasicInfo>>,
    exchange_info: &ExchangeInfo,
) -> Result<ExchangeProjectDetail, Error> {
    // Load basic project data
    let row = sqlx::query!(
        r#"
        SELECT
            name, image, image_credit, goal, task, description,
            how_to_join, presenter, url, twitter_name, facebook_page,
            total_expense, indoors, outdoors, created,
            location_type, location_landuse, location_description,
            city, state, country, address,
            -- Simplified proportionally to its own extent, so a
            -- continent-sized coverage sheds vertices it could not show at
            -- display scale while a single valley keeps its shape.
            AsGeoJSON(
              SimplifyPreserveTopology(
                regions,
                max(
                  max(MbrMaxX(regions) - MbrMinX(regions),
                      MbrMaxY(regions) - MbrMinY(regions)) / 500.0,
                  0.000001
                )
              ),
              6
            ) as "coverage: Option<String>"
        FROM projects
        WHERE project_id = ?
        "#,
        project_id
    )
    .fetch_one(common::shared::db())
    .await?;

    // Load tags
    let tag_rows = sqlx::query!(
        "SELECT tag FROM project_tags WHERE project_id = ? ORDER BY tag",
        project_id
    )
    .fetch_all(common::shared::db())
    .await?;
    let tags: Vec<String> = tag_rows
        .into_iter()
        .map(|r| r.tag)
        .filter(|t| !crate::project::is_hidden_tag(t))
        .collect();

    // Load activities
    let activity_rows = sqlx::query!(
        "SELECT activity FROM project_activities WHERE project_id = ? ORDER BY activity",
        project_id
    )
    .fetch_all(common::shared::db())
    .await?;
    let activities: Vec<(String, String)> = activity_rows
        .into_iter()
        .map(|r| (r.activity.clone(), r.activity))
        .collect();

    // Load topics
    let topic_rows = sqlx::query!(
        "SELECT topic FROM project_topics WHERE project_id = ? ORDER BY topic",
        project_id
    )
    .fetch_all(common::shared::db())
    .await?;
    let topics: Vec<(String, String)> = topic_rows
        .into_iter()
        .map(|r| (r.topic.clone(), r.topic))
        .collect();

    // Load audiences
    let audience_rows = sqlx::query!(
        "SELECT audience FROM project_audiences WHERE project_id = ? ORDER BY audience",
        project_id
    )
    .fetch_all(common::shared::db())
    .await?;
    let audience: Vec<(String, String)> = audience_rows
        .into_iter()
        .map(|r| (r.audience.clone(), r.audience))
        .collect();

    // Like count is cached per-project on this frontend instance; the log-service
    // stays authoritative and unifies orrery-synced history with new hydra likes.
    let project_ulid = Ulid::from_string(project_id)?;
    let likes = crate::component::detail_page::cached_like_count(project_ulid).await?;

    // Get user-specific data
    let (user_is_authorized, liked, saved, joined) = if let Some(user) = user_info {
        let has_permission = access::may_manage(exchange_info.exchange_id, Some(user)).await?;

        // Whether this user has liked the project (live per-user lookup)
        let liked =
            crate::component::detail_page::user_has_liked(project_ulid, user.user_id).await?;

        // Check if user has saved (bookmarked) this project
        use common::message::user::{BookmarkCheckRequest, BookmarkCheckResponse};
        let bookmark_response: BookmarkCheckResponse = request(
            "user.bookmarks.check",
            BookmarkCheckRequest {
                user_id: user.user_id,
                project_id: project_id.to_string(),
            },
        )
        .await?;
        let saved = bookmark_response.has_bookmark;

        // Check if user has joined this project
        use common::message::user::{ProjectJoinCheckRequest, ProjectJoinCheckResponse};
        let join_response: ProjectJoinCheckResponse = request(
            "user.joins.check",
            ProjectJoinCheckRequest {
                user_id: user.user_id,
                project_id: project_id.to_string(),
            },
        )
        .await?;
        let joined = join_response.has_joined;

        (has_permission, liked, saved, joined)
    } else {
        (false, false, false, false)
    };

    // Public reviews are cached per-project on this frontend instance; show the
    // 5 most recent.
    let reviews_response =
        crate::component::detail_page::cached_project_reviews(project_ulid).await?;
    let project_reviews: Vec<_> = reviews_response.reviews.into_iter().take(5).collect();

    // Attach author identity on the shared path, so an exchange page hides
    // private users' reviews exactly as the main detail page does.
    let reviews = crate::component::detail_page::enrich_reviews(project_reviews).await?;

    // Convert empty strings to None for optional fields
    let url = if row.url.is_empty() {
        None
    } else {
        Some(row.url)
    };
    let twitter_name = if row.twitter_name.is_empty() {
        None
    } else {
        Some(row.twitter_name)
    };
    let facebook_page = if row.facebook_page.is_empty() {
        None
    } else {
        Some(row.facebook_page)
    };

    let coverage = row.coverage.flatten();

    let location = Where::new(
        &row.location_type,
        &row.location_landuse,
        &row.location_description,
        coverage.is_some(),
        &row.city,
        &row.state,
        &row.country,
        &row.address,
    );

    // Only offer the polygon where the location actually calls for a map, so
    // an area polygon left behind on a project that has since gone global
    // does not reappear as one.
    let coverage = if location.has_map() { coverage } else { None };

    Ok(ExchangeProjectDetail {
        id: project_slug.to_string(),
        slug: project_slug.to_string(),
        name: row.name,
        image: row.image,
        image_credit: row.image_credit,
        goal: row.goal,
        task: row.task,
        // Render rich-text fields through the sanitizing quill renderer so the
        // template can emit them without exposing stored XSS (the same path the
        // main detail page uses).
        description: content_to_html(&row.description)
            .map(|(html, _)| html)
            .unwrap_or(row.description),
        how_to_join: content_to_html(&row.how_to_join)
            .map(|(html, _)| html)
            .unwrap_or(row.how_to_join),
        presenter: row.presenter,
        url,
        twitter_name,
        facebook_page,
        total_expense: row.total_expense,
        indoors: row.indoors,
        outdoors: row.outdoors,
        tags,
        updated: TimeString::try_from(row.created)?,
        activities,
        topics,
        audience,
        location,
        coverage,
        user_is_authorized,
        liked,
        likes,
        saved,
        joined,
        reviews,
    })
}

/// Exchange home page
#[handler]
pub async fn exchange_home(
    Path(exchange_id): Path<i64>,
    session: &Session,
    auth: Authenticated,
    device: Device,
    interaction: Interaction,
) -> Result<poem::Response, Error> {
    let exchange_info = ExchangeInfo::load_by_id(exchange_id).await?;
    let exchange = create_template_exchange(exchange_info, &auth).await?;

    tracking::record(
        exchange.info.exchange_id,
        auth.0.as_ref().map(|user| user.user_id),
        &device,
        &interaction,
        TrackedPage::Home,
        None,
        TrackedAction::View,
        None,
    )
    .await?;

    // Get CSRF token from session
    let csrftoken = session
        .get::<String>("csrftoken")
        .unwrap_or_else(|| "".to_string());

    // Load predefined searches
    let predefined_searches = exchange.info.get_predefined_searches().await?;
    let predefined: Vec<(String, String)> = predefined_searches
        .iter()
        .map(|s| {
            let phrase = s.search_params.phrase.first().cloned().unwrap_or_default();
            (s.name.clone(), phrase)
        })
        .collect();

    // Load filter options from cache (or database on first use)
    let filter_options = get_filter_options().await?;

    // Load initial project results: no filters, and "include global
    // projects" as the exchange starts it out.
    let params = exchange.info.initial_search();
    let scope = SearchScope::resolve(&params, exchange.user_is_authorized).await?;
    let page = 1;
    let page_size = 20;
    let offset = 0;

    // Build query for initial projects
    let mut query_builder = sqlx::QueryBuilder::new("");
    build_base_where_clause(&mut query_builder, exchange.info.exchange_id, &scope, false);
    apply_search_filters(&mut query_builder, &exchange.info, &params, &scope);

    // Add ordering and pagination
    query_builder.push(" ORDER BY p.created DESC LIMIT ");
    query_builder.push_bind(page_size as i64);
    query_builder.push(" OFFSET ");
    query_builder.push_bind(offset as i64);

    let rows = query_builder
        .build_query_as::<(
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        )>()
        .fetch_all(common::shared::db())
        .await?;

    let mut projects: Vec<ProjectSummary> = rows
        .into_iter()
        .map(
            |(project_id, slug, name, image, image_credit, goal, task, _, _)| ProjectSummary {
                project_id,
                slug,
                name,
                image,
                image_credit,
                goal,
                task,
                location: Where::default(),
            },
        )
        .collect();

    ProjectSummary::attach_locations(&mut projects).await?;

    // Get total count
    let mut count_builder = sqlx::QueryBuilder::new("");
    build_base_where_clause(&mut count_builder, exchange.info.exchange_id, &scope, true);
    apply_search_filters(&mut count_builder, &exchange.info, &params, &scope);

    let (total_count,) = count_builder
        .build_query_as::<(i64,)>()
        .fetch_one(common::shared::db())
        .await?;

    let total = total_count as usize;
    let pagination = Pagination { page, page_size };

    let template = ExchangeIndex {
        exchange: &exchange,
        user: &auth.into(),
        csrftoken,
        predefined,
        activities: filter_options.activities,
        topics: filter_options.topics,
        audiences: filter_options.audiences,
        projects: &projects,
        pagination: &pagination,
        params: &params,
        total,
    };

    Ok(Html(template.render_once()?).into_response())
}

/// Exchange search results
#[handler]
pub async fn exchange_results(
    Path(exchange_id): Path<i64>,
    crate::form::QsQuery(params): crate::form::QsQuery<SearchParams>,
    auth: Authenticated,
    device: Device,
    interaction: Interaction,
) -> Result<poem::Response, Error> {
    let exchange_info = ExchangeInfo::load_by_id(exchange_id).await?;
    let exchange = create_template_exchange(exchange_info, &auth).await?;

    tracking::record(
        exchange.info.exchange_id,
        auth.0.as_ref().map(|user| user.user_id),
        &device,
        &interaction,
        TrackedPage::Results,
        None,
        TrackedAction::Search,
        None,
    )
    .await?;

    // Get pagination from params
    let page = params.page.max(1);
    let page_size = params.page_size.max(1).min(100);
    let offset = (page - 1) * page_size;

    // Build base query using QueryBuilder
    let scope = SearchScope::resolve(&params, exchange.user_is_authorized).await?;
    let has_text_search = scope.has_text_search();
    let mut query_builder = sqlx::QueryBuilder::new("");

    // Build base WHERE clause
    build_base_where_clause(&mut query_builder, exchange.info.exchange_id, &scope, false);

    // Apply common search filters
    apply_search_filters(&mut query_builder, &exchange.info, &params, &scope);

    // Add ordering - use bm25() relevance for text search, otherwise creation date
    if has_text_search {
        query_builder.push(" ORDER BY bm25(projects_fts), p.created DESC");
    } else {
        query_builder.push(" ORDER BY p.created DESC");
    }

    // Add pagination
    query_builder.push(" LIMIT ");
    query_builder.push_bind(page_size as i64);
    query_builder.push(" OFFSET ");
    query_builder.push_bind(offset as i64);

    // Execute query
    let rows = query_builder
        .build_query_as::<(
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        )>()
        .fetch_all(common::shared::db())
        .await?;

    let mut projects: Vec<ProjectSummary> = rows
        .into_iter()
        .map(
            |(project_id, slug, name, image, image_credit, goal, task, _, _)| ProjectSummary {
                project_id,
                slug,
                name,
                image,
                image_credit,
                goal,
                task,
                location: Where::default(),
            },
        )
        .collect();

    ProjectSummary::attach_locations(&mut projects).await?;

    // Build count query with same filters
    let mut count_builder = sqlx::QueryBuilder::new("");

    // Build base WHERE clause for count
    build_base_where_clause(&mut count_builder, exchange.info.exchange_id, &scope, true);

    // Add same filters as main query
    apply_search_filters(&mut count_builder, &exchange.info, &params, &scope);

    let (total_count,) = count_builder
        .build_query_as::<(i64,)>()
        .fetch_one(common::shared::db())
        .await?;

    let total = total_count as usize;

    let pagination = Pagination { page, page_size };

    let template = SearchResults {
        exchange: &exchange,
        projects: &projects,
        pagination: &pagination,
        params: &params,
        total,
    };

    Ok(Html(template.render_once()?).into_response())
}

/// One project in an exchange's CSV export, as anybody may have it.
///
/// The columns are those of `ExchangeResultsRowSimple` in orrery's
/// src/module/exchange.rs, so the export reads the same across the cutover;
/// the values are hydra's own: its project ids, and its slug-based pages.
#[derive(serde::Serialize)]
struct ExchangeCsvRow {
    id: String,
    name: String,
    exchange: String,
    exchange_page: String,
    scistarter_page: String,
}

/// One project in an exchange's CSV export, as the exchange's own people
/// have it. The columns are those of orrery's `ExchangeResultsRowFull`.
#[derive(serde::Serialize)]
struct ExchangeCsvRowFull {
    id: String,
    name: String,
    exchange: String,
    exchange_page: String,
    scistarter_page: String,
    squelched: bool,
    presenter: String,
    url: String,
    origin: String,
    location_type: String,
    point: Option<String>,
    regions: Option<String>,
    contact_name: Option<String>,
    contact_email: Option<String>,
    contact_address: Option<String>,
    created: String,
    updated: Option<String>,
    audience: String,
    classroom_materials: String,
    admin_link: String,
}

/// Exchange CSV export.
///
/// Whatever the search asks for, as a spreadsheet. Anybody may have the
/// short form; the exchange's own people get the long one, with the
/// projects they have hidden included and marked, as orrery does.
#[handler]
pub async fn exchange_csv(
    Path(exchange_id): Path<i64>,
    crate::form::QsQuery(params): crate::form::QsQuery<SearchParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let exchange = ExchangeInfo::load_by_id(exchange_id).await?;
    let full = access::may_manage(exchange.exchange_id, user_info.as_deref()).await?;
    let scope = SearchScope::resolve(&params, full).await?;

    let mut query_builder = sqlx::QueryBuilder::new("");
    build_base_where_clause(&mut query_builder, exchange.exchange_id, &scope, false);
    apply_search_filters(&mut query_builder, &exchange, &params, &scope);

    if scope.has_text_search() {
        query_builder.push(" ORDER BY bm25(projects_fts), p.created DESC");
    } else {
        query_builder.push(" ORDER BY p.created DESC");
    }

    let rows = query_builder
        .build_query_as::<(
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        )>()
        .fetch_all(common::shared::db())
        .await?;

    let hidden = if full {
        Exchange::load(exchange.exchange_id).await?.hidden_projects
    } else {
        Default::default()
    };

    // When each project was last updated, as its page says.
    let updates = if full {
        last_changes(
            rows.iter()
                .filter_map(|row| Ulid::from_string(&row.0).ok())
                .collect(),
        )
        .await?
    } else {
        Default::default()
    };

    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(vec![]);

    for (project_id, slug, name, _, _, _, _, submitted_through, admin_link) in rows {
        let row = ExchangeCsvRow {
            exchange_page: format!(
                "https://scistarter.org/exchange/{}/{slug}/project.html",
                exchange.path_id
            ),
            scistarter_page: format!("https://scistarter.org/projects/{slug}"),
            id: project_id,
            name,
            exchange: submitted_through,
        };

        if !full {
            writer.serialize(row)?;
            continue;
        }

        let project_id = Ulid::from_string(&row.id)?;
        let project = ProjectAll::load(project_id).await?;
        let contact = project
            .contacts
            .iter()
            .find(|contact| matches!(contact.role, crate::contact::ContactRole::ContactPerson));

        writer.serialize(ExchangeCsvRowFull {
            id: row.id,
            name: row.name,
            exchange: row.exchange,
            exchange_page: row.exchange_page,
            scistarter_page: row.scistarter_page,
            squelched: hidden.contains_key(&project_id),
            presenter: project.presenter,
            url: project.url,
            origin: project.origin,
            location_type: project.location_type,
            point: project.point,
            regions: project.regions,
            contact_name: contact.and_then(|contact| contact.name.clone()),
            contact_email: contact.map(|contact| contact.email.clone()),
            contact_address: contact.and_then(|contact| contact.address.clone()),
            created: project.created.to_string(),
            updated: updates.get(&project_id).map(|when| when.to_string()),
            audience: project.audience.join(";"),
            classroom_materials: project.classroom_materials,
            admin_link,
        })?;
    }

    Ok(poem::Response::builder()
        .header("Content-Type", "text/csv")
        .header(
            "Content-Disposition",
            "attachment; filename=\"projects.csv\"",
        )
        .body(writer.into_inner()?))
}

#[derive(TemplateOnce)]
#[template(path = "exchange/org.stpl.html")]
struct ExchangeOrgPage {
    exchange: Exchange,
    template_exchange: TemplateExchange,
    user: TemplateUser,
    csrftoken: String,
    regions: Vec<(i64, String)>,
    owner: Option<StaffMember>,
    staff: Vec<StaffMember>,
    pending: Vec<PendingSubmission>,
}

#[derive(Clone)]
struct StaffMember {
    user_id: Ulid,
    name: String,
    email: String,
}

impl StaffMember {
    fn to_html(&self, exchange_id: i64) -> String {
        // Use simple HTML entity encoding for quotes and angle brackets
        let name = self
            .name
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");
        let email = self
            .email
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");

        format!(
            r#"<div class="org-item oi-line">
                <span class="label">{}</span>
                <p><a href="mailto:{}">{}</a></p>
                <button type="button" hx-delete="/exchange/{}/staff/{}" hx-target="closest .org-item" hx-swap="outerHTML" class="oi-remove-item oi-actions">remove</button>
            </div>"#,
            name, email, email, exchange_id, self.user_id
        )
    }
}

/// What can be submitted through an exchange and wait for its approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionKind {
    Project,
    Event,
}

impl SubmissionKind {
    /// The path segment naming the kind in approve and reject URLs.
    fn segment(self) -> &'static str {
        match self {
            SubmissionKind::Project => "projects",
            SubmissionKind::Event => "events",
        }
    }

    fn from_segment(segment: &str) -> Option<SubmissionKind> {
        match segment {
            "projects" => Some(SubmissionKind::Project),
            "events" => Some(SubmissionKind::Event),
            _ => None,
        }
    }

    fn entity_kind(self) -> crate::entity::EntityKind {
        match self {
            SubmissionKind::Project => crate::entity::EntityKind::Project,
            SubmissionKind::Event => crate::entity::EntityKind::Event,
        }
    }
}

/// A project or event waiting in an exchange's approval queue.
struct PendingSubmission {
    kind: SubmissionKind,
    id: Ulid,
    name: String,
    slug: Option<String>,
}

impl PendingSubmission {
    fn to_html(&self, exchange_id: i64) -> String {
        let name = html_escape(&self.name);

        // Each opens on the exchange's own page for it.
        let page = match self.kind {
            SubmissionKind::Project => "project.html",
            SubmissionKind::Event => "event.html",
        };
        let url = match &self.slug {
            Some(slug) => format!("/exchange/{}/{}/{}", exchange_id, html_escape(slug), page),
            None => format!("/exchange/{}/{}/{}", exchange_id, self.id, page),
        };
        let kind = self.kind.segment();

        format!(
            r#"<div class="org-item oi-line" style="gap: 8px;">
                <span class="label"><a href="{url}" target="_blank" style="color: inherit; text-decoration: underline;">{name}</a></span>
                <div style="display: flex; gap: 8px;">
                    <button type="button" style="width: auto; min-width: 80px; height: 32px; padding: 0 12px; line-height: 1;" hx-post="/exchange/{exchange_id}/{kind}/{id}/approve" hx-target="closest .org-item" hx-swap="outerHTML" class="el-button el-button--primary">Approve</button>
                    <button type="button" style="width: auto; min-width: 80px; height: 32px; padding: 0 12px; line-height: 1;" hx-delete="/exchange/{exchange_id}/{kind}/{id}/reject" hx-target="closest .org-item" hx-swap="outerHTML" class="el-button el-button--danger">Reject</button>
                </div>
            </div>"#,
            id = self.id,
        )
    }
}

fn percent(num: i32, den: i32) -> i32 {
    if num == 0 {
        0
    } else if den == 0 {
        100
    } else {
        (((num as f32) / (den as f32)) * 100.0) as i32
    }
}

#[derive(Default)]
struct DataOverview {
    pub entity_count: i32,
    pub in_person_count: i32,
    pub online_count: i32,
    pub global_count: i32,
    pub near_count: i32,
    pub at_count: i32,
}

impl DataOverview {
    async fn load_for_exchange(exchange_id: Ulid) -> Result<Self, Error> {
        let exchange_id_str = exchange_id.to_string();

        // Count total published projects associated with this exchange
        let entity_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) as "count!: i32"
            FROM projects p
            INNER JOIN entities e ON e.target = p.project_id AND e.kind = 'Project' AND e.publish = 1
            WHERE p.exchange = ?
            "#,
            exchange_id_str
        )
        .fetch_one(db())
        .await?;

        // Count in-person projects (where in_person = true)
        let in_person_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) as "count!: i32"
            FROM projects p
            INNER JOIN entities e ON e.target = p.project_id AND e.kind = 'Project' AND e.publish = 1
            WHERE p.exchange = ? AND p.in_person = 1
            "#,
            exchange_id_str
        )
        .fetch_one(db())
        .await?;

        // Count online-only projects (where entirely_online = true AND in_person = false)
        let online_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) as "count!: i32"
            FROM projects p
            INNER JOIN entities e ON e.target = p.project_id AND e.kind = 'Project' AND e.publish = 1
            WHERE p.exchange = ? AND p.entirely_online = 1 AND (p.in_person = 0 OR p.in_person IS NULL)
            "#,
            exchange_id_str
        )
        .fetch_one(db())
        .await?;

        // Count by geographic scope based on location_type
        let global_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) as "count!: i32"
            FROM projects p
            INNER JOIN entities e ON e.target = p.project_id AND e.kind = 'Project' AND e.publish = 1
            WHERE p.exchange = ? AND p.location_type = 'Global, anywhere on the planet'
            "#,
            exchange_id_str
        )
        .fetch_one(db())
        .await?;

        let near_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) as "count!: i32"
            FROM projects p
            INNER JOIN entities e ON e.target = p.project_id AND e.kind = 'Project' AND e.publish = 1
            WHERE p.exchange = ? AND p.location_type = 'In a geographic location or range'
            "#,
            exchange_id_str
        )
        .fetch_one(db())
        .await?;

        let at_count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) as "count!: i32"
            FROM projects p
            INNER JOIN entities e ON e.target = p.project_id AND e.kind = 'Project' AND e.publish = 1
            WHERE p.exchange = ? AND p.location_type = 'At a specific address'
            "#,
            exchange_id_str
        )
        .fetch_one(db())
        .await?;

        Ok(DataOverview {
            entity_count,
            in_person_count,
            online_count,
            global_count,
            near_count,
            at_count,
        })
    }
}

/// Exchange data dashboard page
#[handler]
pub async fn exchange_data(
    Path(exchange_id): Path<i64>,
    session: &poem::session::Session,
    auth: Authenticated,
    device: Device,
    interaction: Interaction,
) -> Result<poem::Response, Error> {
    let exchange_info = ExchangeInfo::load_by_id(exchange_id).await?;
    let exchange = create_template_exchange(exchange_info, &auth).await?;

    tracking::record(
        exchange.info.exchange_id,
        auth.0.as_ref().map(|user| user.user_id),
        &device,
        &interaction,
        TrackedPage::Data,
        None,
        TrackedAction::View,
        None,
    )
    .await?;

    // Get CSRF token
    let csrftoken = session
        .get::<String>("csrftoken")
        .unwrap_or_else(|| "".to_string());

    // Load actual data from database
    let overview = DataOverview::load_for_exchange(exchange.info.exchange_id).await?;

    #[derive(TemplateOnce)]
    #[template(path = "exchange/data.stpl.html")]
    struct ExchangeDataPage<'a> {
        exchange: &'a TemplateExchange,
        user: &'a TemplateUser,
        csrftoken: String,
        overview: DataOverview,
    }

    impl ExchangeDataPage<'_> {
        fn encode(&self, s: &str) -> String {
            urlencoding::encode(s).to_string()
        }
    }

    let page = ExchangeDataPage {
        exchange: &exchange,
        user: &TemplateUser(auth.0),
        csrftoken,
        overview,
    };

    Ok(Html(page.render_once()?).into_response())
}

/// Exchange engagement tab (HTMX endpoint)
#[handler]
pub async fn exchange_engagement(
    Path(exchange_id): Path<i64>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<poem::Response, Error> {
    use common::message::log::{ExchangeEngagementRequest, ExchangeEngagementResponse};
    use common::service::request;

    let exchange_info = ExchangeInfo::load_by_id(exchange_id).await?;

    // Parse year from query params
    let year = params
        .get("year")
        .and_then(|y| y.parse::<i32>().ok())
        .filter(|y| *y > 0);

    // Request engagement data from log service
    let response: ExchangeEngagementResponse = request(
        "log.exchange_engagement",
        ExchangeEngagementRequest {
            exchange_id: exchange_info.exchange_id.to_string(),
            year,
            project_ids: listed_project_ids(&exchange_info).await?,
        },
    )
    .await?;

    // Get project details for top projects
    let top_views = fetch_project_details(&response.top_views).await?;
    let top_saves = fetch_project_details(&response.top_saves).await?;
    let top_referrals = fetch_project_details(&response.top_referrals).await?;

    type TopList = Vec<(String, String, String, i32)>;

    #[derive(TemplateOnce)]
    #[template(path = "exchange/engagement.stpl.html")]
    struct EngagementContent {
        exchange_id: i64,
        selected_year: i32,
        selected_year_display: String,
        available_years: Vec<i32>,
        exchange_visits: i32,
        visitors: i32,
        views: i32,
        saves: i32,
        referrals: i32,
        contributions: i32,
        top_views: TopList,
        top_saves: TopList,
        top_referrals: TopList,
        visits_since: Option<String>,
    }

    impl EngagementContent {
        /// The top-ten lists, each with its tab id and label.
        fn top_lists(&self) -> [(&'static str, &'static str, &TopList); 3] {
            [
                ("views", "Views", &self.top_views),
                ("saves", "Saves", &self.top_saves),
                ("referrals", "Referrals", &self.top_referrals),
            ]
        }
    }

    let selected_year_display = if let Some(y) = year {
        y.to_string()
    } else {
        "All Time".to_string()
    };

    let content = EngagementContent {
        exchange_id,
        selected_year: year.unwrap_or(0),
        selected_year_display,
        available_years: response.available_years,
        exchange_visits: response.exchange_visits,
        visitors: response.visitors,
        views: response.views,
        saves: response.saves,
        referrals: response.referrals,
        contributions: response.contributions,
        top_views,
        top_saves,
        top_referrals,
        visits_since: tracking_note(response.tracked_since.as_deref(), year),
    };

    Ok(Html(content.render_once()?).into_response())
}

/// The note visits and visitors carry, worded as orrery's `tracking_note`
/// words it: from when they run, if the period asked about began earlier,
/// or that they have not begun.
fn tracking_note(tracked_since: Option<&str>, year: Option<i32>) -> Option<String> {
    let Some(since) = tracked_since.and_then(|since| since.parse::<jiff::Timestamp>().ok()) else {
        return Some("not yet tracked".into());
    };

    let since = since.to_zoned(jiff::tz::TimeZone::UTC);
    let period_began_earlier = match year {
        Some(year) => {
            i32::from(since.year()) > year
                || (i32::from(since.year()) == year && since.day_of_year() > 1)
        }
        None => true,
    };

    period_began_earlier.then(|| format!("since {}", since.strftime("%B %-d, %Y")))
}

/// The ids of the projects an exchange lists by default -- the projects
/// whose contributions its data page counts, as orrery counts those its own
/// default search finds.
async fn listed_project_ids(exchange: &ExchangeInfo) -> Result<Vec<String>, Error> {
    let params = exchange.initial_search();
    let scope = SearchScope::resolve(&params, false).await?;

    let mut query_builder = sqlx::QueryBuilder::new("SELECT listed.project_id FROM (");
    build_base_where_clause(&mut query_builder, exchange.exchange_id, &scope, false);
    apply_search_filters(&mut query_builder, exchange, &params, &scope);
    query_builder.push(") listed");

    Ok(query_builder
        .build_query_scalar::<String>()
        .fetch_all(db())
        .await?)
}

async fn fetch_project_details(
    project_counts: &[common::message::log::ProjectCount],
) -> Result<Vec<(String, String, String, i32)>, Error> {
    use sqlx::Row;

    let mut results = Vec::new();

    for pc in project_counts {
        // Query project details from database
        // The goal, as orrery's top lists show it, not the description, which
        // is long rich text.
        let row = sqlx::query(
            r#"
            SELECT name, goal, image
            FROM projects
            WHERE project_id = ?
            "#,
        )
        .bind(&pc.project_id)
        .fetch_optional(db())
        .await?;

        if let Some(row) = row {
            let name: String = row
                .try_get("name")
                .unwrap_or_else(|_| "Unknown Project".to_string());
            let goal: String = row.try_get("goal").unwrap_or_default();
            let image: String = row.try_get("image").unwrap_or_default();

            results.push((name, goal, image, pc.count));
        }
    }

    Ok(results)
}

/// Exchange admin page
#[handler]
pub async fn exchange_admin(
    Path(exchange_id): Path<i64>,
    session: &poem::session::Session,
    Authenticated(user_info): Authenticated,
    device: Device,
    interaction: Interaction,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let exchange = ExchangeInfo::load_by_id(exchange_id).await?;

    let has_permission = access::may_manage(exchange.exchange_id, Some(&*user_info)).await?;

    if !has_permission {
        return Ok(error_response(poem::http::StatusCode::FORBIDDEN, ""));
    }

    tracking::record(
        exchange.exchange_id,
        Some(user_info.user_id),
        &device,
        &interaction,
        TrackedPage::Org,
        None,
        TrackedAction::View,
        None,
    )
    .await?;

    // Get CSRF token
    let csrftoken = session
        .get::<String>("csrftoken")
        .unwrap_or_else(|| "".to_string());

    // Offered as the exchange's own scope: the areas a search can be limited
    // to are the same list a project's coverage is measured against.
    let regions = crate::region::all().await?;

    // Load full exchange for editing
    let exchange_full = Exchange::load(exchange.exchange_id).await?;

    // Create template exchange for navigation
    let template_exchange = TemplateExchange {
        info: exchange.clone(),
        user_is_authorized: true, // Already checked above
    };

    let people = access::people(exchange.exchange_id).await?;

    let mut owner = None;
    let mut staff = Vec::new();
    if !people.is_empty() {
        match request::<common::message::user::UserDetailsBatchGetResponse>(
            "user.admin.user_batch_get",
            common::message::user::UserDetailsBatchGetRequest {
                user_ids: people.iter().map(|(user_id, _)| *user_id).collect(),
            },
        )
        .await
        {
            Ok(user_batch_response) => {
                for user_mapping in user_batch_response.users {
                    let member = StaffMember {
                        user_id: user_mapping.user_id,
                        name: user_mapping.primary_email.clone(), // Use email as name for now
                        email: user_mapping.primary_email,
                    };

                    if people.contains(&(member.user_id, access::ExchangeRole::Owner)) {
                        owner = Some(member);
                    } else {
                        staff.push(member);
                    }
                }
            }
            Err(_e) => {
                // Failed to fetch user details, but continue
            }
        }
    }

    // The approval queue: projects and events submitted through this
    // exchange that it has not yet approved or rejected.
    let exchange_id_str = exchange.exchange_id.to_string();
    let pending_rows = sqlx::query!(
        r#"
        SELECT 'projects' AS "kind!: String", p.project_id AS "id!: String", p.name, e.slug
        FROM projects p
        LEFT JOIN entities e ON e.kind = 'Project' AND e.target = p.project_id
        WHERE p.exchange = ?1 AND p.reviewed = 0
        UNION ALL
        SELECT 'events', ev.event_id, ev.name, e.slug
        FROM events ev
        LEFT JOIN entities e ON e.kind = 'Event' AND e.target = ev.event_id
        WHERE ev.exchange = ?1 AND ev.reviewed = 0
        ORDER BY 3
        "#,
        exchange_id_str
    )
    .fetch_all(db())
    .await?;

    let pending: Vec<PendingSubmission> = pending_rows
        .into_iter()
        .filter_map(|row| {
            Some(PendingSubmission {
                kind: SubmissionKind::from_segment(&row.kind)?,
                id: Ulid::from_string(&row.id).ok()?,
                name: row.name,
                slug: row.slug,
            })
        })
        .collect();

    let page = ExchangeOrgPage {
        exchange: exchange_full,
        template_exchange,
        user: TemplateUser(Some(user_info)),
        csrftoken,
        regions,
        owner,
        staff,
        pending,
    };

    Ok(Html(page.render_once()?).into_response())
}

/// What an exchange's own people may change about which projects it lists,
/// matching orrery's org settings form.
#[derive(Debug, serde::Deserialize)]
struct ExchangeSettingsForm {
    name: String,
    #[serde(default)]
    exclude_scistarter: bool,
    #[serde(default)]
    search: String,
    #[serde(default)]
    restricted_add: bool,
    #[serde(default)]
    global_by_default: bool,
    /// Empty means "no region": a view of the whole catalogue.
    #[serde(default)]
    region_id: String,
}

/// Why `search` cannot be an exchange's search setting, if it cannot.
///
/// Checked with the same engine SQLite's `REGEXP` runs it with, so anything
/// accepted here will run, and nothing that would make every search on the
/// exchange fail is ever stored.
pub fn search_error(search: &str) -> Option<String> {
    regex::Regex::new(&search_pattern(search))
        .err()
        .map(|error| error.to_string())
}

/// Update exchange settings (uses LoadStorePeered for write)
#[handler]
pub async fn exchange_update_settings(
    Path(path_id): Path<i64>,
    form: poem::web::Form<ExchangeSettingsForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let info = ExchangeInfo::load_by_id(path_id).await?;

    if !access::may_manage(info.exchange_id, Some(&*user_info)).await? {
        return Ok(error_response(poem::http::StatusCode::FORBIDDEN, ""));
    }

    let search = form.search.trim();
    if let Some(error) = search_error(search) {
        return Ok(Html(format!(
            r#"<span style="color: #b91c1c">That search is not a usable regular expression: {}</span>"#,
            html_escape(&error)
        ))
        .into_response());
    }

    let mut exchange = Exchange::load(info.exchange_id).await?;

    exchange.name = form.name.trim().to_string();
    exchange.settings.exclude_scistarter = form.exclude_scistarter;
    exchange.settings.search = search.to_string();
    exchange.settings.restricted_add = form.restricted_add;
    exchange.settings.global_by_default = form.global_by_default;
    exchange.settings.region_id = form.region_id.trim().parse().ok();

    // Use LoadStorePeered to save and broadcast to peers
    exchange.store().await?;

    Ok(Html(r#"<span style="color: #007974">Saved!</span>"#).into_response())
}

#[derive(Debug, serde::Deserialize)]
struct PredefinedSearchForm {
    name: String,
    #[serde(default)]
    phrase: String, // Comma-separated phrases
    #[serde(default)]
    activity: String,
    #[serde(default)]
    topic: String,
    #[serde(default)]
    audience: String, // Comma-separated audience types
}

/// Get empty predefined search form row
#[handler]
pub async fn get_predefined_search_form(
    Path(exchange_id): Path<i64>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let exchange = ExchangeInfo::load_by_id(exchange_id).await?;
    let has_permission = access::may_manage(exchange.exchange_id, Some(&*user_info)).await?;

    if !has_permission {
        return Ok(error_response(poem::http::StatusCode::FORBIDDEN, ""));
    }

    // Return an editable form row
    let html = format!(
        r#"<div style="gap: 8px;">
            <span></span>
            <input type="text" name="name" placeholder="Label" />
            <input type="text" name="phrase" placeholder="Search terms" />
            <button type="button" style="width: auto; min-width: 80px; height: 32px; padding: 0 12px; line-height: 1;" hx-post="/exchange/{}/predefined-search" hx-include="closest div" hx-target="closest div" hx-swap="outerHTML">Save</button>
            <button type="button" style="width: auto; min-width: 80px; height: 32px; padding: 0 12px; line-height: 1;" onclick="this.closest('div').remove()">Cancel</button>
        </div>"#,
        exchange_id
    );

    Ok(Html(html).into_response())
}

/// Create predefined search (uses LoadStorePeered for write)
#[handler]
pub async fn create_predefined_search(
    Path(path_id): Path<i64>,
    form: poem::web::Form<PredefinedSearchForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    // Look up exchange_id from path_id
    let exchange_id_str = sqlx::query_scalar!(
        "SELECT exchange_id FROM exchanges WHERE path_id = ?",
        path_id
    )
    .fetch_one(db())
    .await?;

    let exchange_id = Ulid::from_string(&exchange_id_str)?;
    let mut exchange = Exchange::load(exchange_id).await?;

    let has_permission = access::may_manage(exchange.exchange_id, Some(&*user_info)).await?;

    if !has_permission {
        return Ok(error_response(poem::http::StatusCode::FORBIDDEN, ""));
    }

    // Parse form data and create predefined search
    let phrases = super::PredefinedSearch::phrases(&form.phrase);

    let audiences: Vec<String> = form
        .audience
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let search_params = SearchParams {
        phrase: phrases,
        activity: if form.activity.is_empty() {
            None
        } else {
            Some(form.activity.clone())
        },
        topic: if form.topic.is_empty() {
            None
        } else {
            Some(form.topic.clone())
        },
        audience: audiences,
        inactive: false,
        // A prompted search is ticked within the finder form, whose own
        // "include global projects" box decides this.
        global: false,
        page: 1,
        page_size: 20,
    };

    // Get next display order (append to end)
    let next_order = exchange.predefined_searches.len() as i32;

    let new_search = super::PredefinedSearch {
        search_id: Ulid::new(),
        name: form.name.clone(),
        search_params,
        display_order: next_order,
    };

    let search_id = new_search.search_id;
    let search_name = new_search.name.clone();
    let phrase = new_search
        .search_params
        .phrase
        .first()
        .cloned()
        .unwrap_or_default();

    exchange.predefined_searches.push(new_search);
    exchange.store().await?;

    // Return HTML for the new search item
    let html = format!(
        r#"<div style="gap: 8px;">
            <span></span>
            <input type="text" value="{}" readonly />
            <input type="text" value="{}" readonly />
            <button type="button" style="width: auto; min-width: 80px; height: 32px; padding: 0 12px; line-height: 1;" hx-delete="/exchange/{}/predefined-search/{}" hx-target="closest div" hx-swap="outerHTML">Delete</button>
        </div>"#,
        html_escape(&search_name),
        html_escape(&phrase),
        path_id,
        search_id
    );

    Ok(Html(html).into_response())
}

/// Delete predefined search (uses LoadStorePeered for write)
#[handler]
pub async fn delete_predefined_search(
    Path((path_id, search_id_str)): Path<(i64, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let search_id = Ulid::from_string(&search_id_str)?;

    // Look up exchange_id from path_id
    let exchange_id_str = sqlx::query_scalar!(
        "SELECT exchange_id FROM exchanges WHERE path_id = ?",
        path_id
    )
    .fetch_one(db())
    .await?;

    let exchange_id = Ulid::from_string(&exchange_id_str)?;
    let mut exchange = Exchange::load(exchange_id).await?;

    let has_permission = access::may_manage(exchange.exchange_id, Some(&*user_info)).await?;

    if !has_permission {
        return Ok(error_response(poem::http::StatusCode::FORBIDDEN, ""));
    }

    // Remove the predefined search
    exchange
        .predefined_searches
        .retain(|s| s.search_id != search_id);

    exchange.store().await?;

    // Return empty content - HTMX will swap and remove the element
    Ok(Html("").into_response())
}

#[derive(Debug, serde::Deserialize)]
struct AddStaffForm {
    emails: String,
}

/// Add staff members to exchange
#[handler]
pub async fn add_exchange_staff(
    Path(exchange_id): Path<i64>,
    form: poem::web::Form<AddStaffForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let exchange = ExchangeInfo::load_by_id(exchange_id).await?;
    let has_permission = access::may_manage(exchange.exchange_id, Some(&*user_info)).await?;

    if !has_permission {
        return Ok(error_response(poem::http::StatusCode::FORBIDDEN, ""));
    }

    // Parse emails from comma-separated list
    let emails: Vec<String> = form
        .emails
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let mut added_staff = Vec::new();

    for email in emails {
        // Look up user by email
        if let Ok(user_response) = request::<common::message::user::UserByEmailResponse>(
            "user.search.by_email",
            common::message::user::UserByEmailRequest {
                email: email.clone(),
            },
        )
        .await
        {
            if user_response.found {
                if let (Some(user_id_str), Some(first_name), Some(last_name), Some(found_email)) = (
                    user_response.user_id,
                    user_response.first_name,
                    user_response.last_name,
                    user_response.email,
                ) {
                    if let Ok(user_ulid) = Ulid::from_string(&user_id_str) {
                        if access::grant(
                            exchange.exchange_id,
                            user_ulid,
                            access::ExchangeRole::Staff,
                            &user_info.user_id.to_string(),
                        )
                        .await
                        .is_err()
                        {
                            continue; // Skip adding to the list if grant failed
                        }

                        let display_name =
                            format!("{} {}", first_name, last_name).trim().to_string();
                        let display_name = if display_name.is_empty() {
                            user_response.handle.unwrap_or_default()
                        } else {
                            display_name
                        };

                        added_staff.push(StaffMember {
                            user_id: user_ulid,
                            name: display_name,
                            email: found_email,
                        });
                    }
                }
            }
        }
    }

    // Render all staff members as HTML
    let html_output: String = added_staff
        .iter()
        .map(|member| member.to_html(exchange_id))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(Html(html_output).into_response())
}

/// Remove staff member from exchange
#[handler]
pub async fn remove_exchange_staff(
    Path((exchange_id, user_id_str)): Path<(i64, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let exchange = ExchangeInfo::load_by_id(exchange_id).await?;
    let has_permission = access::may_manage(exchange.exchange_id, Some(&*user_info)).await?;

    if !has_permission {
        return Ok(error_response(poem::http::StatusCode::FORBIDDEN, ""));
    }

    access::revoke(exchange.exchange_id, Ulid::from_string(&user_id_str)?).await?;

    // Return empty response (HTMX will remove the element)
    Ok(Html("").into_response())
}

/// The name of a submission, if it was submitted through `exchange_id`.
async fn submission_through(
    kind: SubmissionKind,
    id: Ulid,
    exchange_id: Ulid,
) -> Result<Option<String>, Error> {
    let id = id.to_string();
    let exchange_id = exchange_id.to_string();

    let name = match kind {
        SubmissionKind::Project => {
            sqlx::query_scalar!(
                "SELECT name FROM projects WHERE project_id = ? AND exchange = ?",
                id,
                exchange_id
            )
            .fetch_optional(db())
            .await?
        }
        SubmissionKind::Event => {
            sqlx::query_scalar!(
                "SELECT name FROM events WHERE event_id = ? AND exchange = ?",
                id,
                exchange_id
            )
            .fetch_optional(db())
            .await?
        }
    };

    Ok(name)
}

/// File a submission as reviewed, which takes it out of the approval queue.
async fn mark_reviewed(kind: SubmissionKind, id: Ulid) -> Result<(), Error> {
    match kind {
        SubmissionKind::Project => {
            use crate::project::field::{ProjectField, ProjectFieldValue};
            ProjectField::new(id, ProjectFieldValue::Reviewed(true))
                .store()
                .await
        }
        SubmissionKind::Event => {
            use crate::event::field::{EventField, EventFieldValue};
            EventField::new(id, EventFieldValue::Reviewed(true))
                .store()
                .await
        }
    }
}

/// Approve a project or event submitted through an exchange, publishing it.
#[handler]
pub async fn approve_submission(
    Path((path_id, kind, id)): Path<(i64, String, String)>,
    Authenticated(user_info): Authenticated,
    device: Device,
    interaction: Interaction,
) -> Result<poem::Response, Error> {
    use crate::entity::Entity;

    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let exchange = ExchangeInfo::load_by_id(path_id).await?;

    if !access::may_manage(exchange.exchange_id, Some(&*user_info)).await? {
        return Ok(error_response(poem::http::StatusCode::FORBIDDEN, ""));
    }

    let Some(kind) = SubmissionKind::from_segment(&kind) else {
        return Ok(error_response(poem::http::StatusCode::NOT_FOUND, ""));
    };
    let id = Ulid::from_string(&id)?;

    let Some(name) = submission_through(kind, id, exchange.exchange_id).await? else {
        return Ok(error_response(
            poem::http::StatusCode::BAD_REQUEST,
            "That was not submitted through this exchange",
        ));
    };

    Entity::publish_page(kind.entity_kind(), id, &name).await?;
    mark_reviewed(kind, id).await?;

    tracking::record(
        exchange.exchange_id,
        Some(user_info.user_id),
        &device,
        &interaction,
        TrackedPage::Org,
        Some(id),
        TrackedAction::Approve,
        None,
    )
    .await?;

    tracing::info!(%id, ?kind, path_id, "Approved submission for exchange");

    let _ = common::service::log_activity(
        None,
        "exchange-submission-approve",
        user_info.user_id,
        Some(id),
        format!("exchange={}", path_id),
    )
    .await;

    // Return empty response (HTMX will remove the element)
    Ok(Html("").into_response())
}

/// Reject a project or event submitted through an exchange.
///
/// It stays unpublished and keeps its association with the exchange, but
/// leaves the approval queue.
#[handler]
pub async fn reject_submission(
    Path((path_id, kind, id)): Path<(i64, String, String)>,
    Authenticated(user_info): Authenticated,
    device: Device,
    interaction: Interaction,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let exchange = ExchangeInfo::load_by_id(path_id).await?;

    if !access::may_manage(exchange.exchange_id, Some(&*user_info)).await? {
        return Ok(error_response(poem::http::StatusCode::FORBIDDEN, ""));
    }

    let Some(kind) = SubmissionKind::from_segment(&kind) else {
        return Ok(error_response(poem::http::StatusCode::NOT_FOUND, ""));
    };
    let id = Ulid::from_string(&id)?;

    if submission_through(kind, id, exchange.exchange_id)
        .await?
        .is_none()
    {
        return Ok(error_response(
            poem::http::StatusCode::BAD_REQUEST,
            "That was not submitted through this exchange",
        ));
    }

    mark_reviewed(kind, id).await?;

    tracking::record(
        exchange.exchange_id,
        Some(user_info.user_id),
        &device,
        &interaction,
        TrackedPage::Org,
        Some(id),
        TrackedAction::Reject,
        None,
    )
    .await?;

    tracing::info!(%id, ?kind, path_id, "Rejected submission for exchange (kept association)");

    let _ = common::service::log_activity(
        None,
        "exchange-submission-reject",
        user_info.user_id,
        Some(id),
        format!("exchange={}", path_id),
    )
    .await;

    // Return empty response (HTMX will remove the element)
    Ok(Html("").into_response())
}

/// Exchange project detail page
#[handler]
pub async fn exchange_project(
    Path((exchange_id, project_slug)): Path<(i64, String)>,
    auth: Authenticated,
    device: Device,
    interaction: Interaction,
    session: &Session,
) -> Result<poem::Response, Error> {
    let exchange_info = ExchangeInfo::load_by_id(exchange_id).await?;
    let exchange = create_template_exchange(exchange_info.clone(), &auth).await?;

    // Load project by slug - include unpublished if user is authorized
    let project_row = sqlx::query!(
        r#"
        SELECT p.project_id, COALESCE(ent.publish, 0) as "publish!: bool"
        FROM projects p
        LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
        WHERE ent.slug = ?
        "#,
        project_slug
    )
    .fetch_optional(common::shared::db())
    .await?;

    let Some(project_row) = project_row else {
        return Ok(match orrery_project_slug(db(), &project_slug).await? {
            Some(slug) => poem::web::Redirect::permanent(format!(
                "/exchange/{exchange_id}/{slug}/project.html"
            ))
            .into_response(),
            None => error_response(poem::http::StatusCode::NOT_FOUND, ""),
        });
    };

    // If project is not published, check if user is authorized for the exchange
    if !project_row.publish && !exchange.user_is_authorized {
        return Ok(error_response(poem::http::StatusCode::NOT_FOUND, ""));
    }

    let project_id = project_row.project_id;

    tracking::record(
        exchange.info.exchange_id,
        auth.0.as_ref().map(|user| user.user_id),
        &device,
        &interaction,
        TrackedPage::Project,
        Some(Ulid::from_string(&project_id)?),
        TrackedAction::View,
        None,
    )
    .await?;

    // Load full project data
    let project =
        load_exchange_project_detail(&project_id, &project_slug, auth.0.as_ref(), &exchange_info)
            .await?;

    // Get CSRF token
    let csrftoken = session
        .get::<String>("csrf-token")
        .unwrap_or_else(|| "".to_string());

    let squelch = if exchange.user_is_authorized {
        let hidden = Exchange::load(exchange_info.exchange_id)
            .await?
            .hidden_projects
            .remove(&Ulid::from_string(&project_id)?);

        Some(
            SquelchButton {
                exchange: exchange_info.path_id,
                project: project_slug,
                hidden,
            }
            .render_once()?,
        )
    } else {
        None
    };

    let user = TemplateUser::from(auth);

    let template = ExchangeProjectPage {
        exchange,
        user,
        project,
        csrftoken,
        squelch,
    };

    Ok(Html(template.render_once()?).into_response())
}

/// Save project to user's bookmarks
#[handler]
pub async fn save_project(
    Path((exchange_id, project_slug)): Path<(i64, String)>,
    Authenticated(user_info): Authenticated,
    device: Device,
    interaction: Interaction,
) -> Result<poem::Response, Error> {
    use common::message::user::{BookmarkCreateRequest, BookmarkCreateResponse};

    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let exchange = ExchangeInfo::load_by_id(exchange_id).await?;
    let project_id = project_by_slug(&project_slug)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Project not found"))?;

    // Save bookmark via user-service
    let response: BookmarkCreateResponse = request(
        "user.bookmarks.create",
        BookmarkCreateRequest {
            user_id: user_info.user_id,
            project_id: project_id.to_string(),
        },
    )
    .await?;

    tracking::record(
        exchange.exchange_id,
        Some(user_info.user_id),
        &device,
        &interaction,
        TrackedPage::Project,
        Some(project_id),
        TrackedAction::Save,
        None,
    )
    .await?;

    if response.success {
        Ok(Html("Project saved").into_response())
    } else {
        Ok(Html(format!("Error: {}", response.message)).into_response())
    }
}

/// Vote on project
#[handler]
pub async fn vote_project(
    Path((exchange_id, project_slug)): Path<(i64, String)>,
    Authenticated(user_info): Authenticated,
    device: Device,
    interaction: Interaction,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let exchange = ExchangeInfo::load_by_id(exchange_id).await?;

    let project_id = project_by_slug(&project_slug)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Project not found"))?;

    // Record the like in the log-service (single source of truth for likes)
    let _: ProjectLikesResponse = request(
        "log.rating_set",
        RatingSetRequest {
            project_id: project_id.to_string(),
            user_id: user_info.user_id.to_string(),
            liked: true,
        },
    )
    .await?;

    tracking::record(
        exchange.exchange_id,
        Some(user_info.user_id),
        &device,
        &interaction,
        TrackedPage::Project,
        Some(project_id),
        TrackedAction::Vote,
        None,
    )
    .await?;

    Ok(Html("Voted").into_response())
}

/// The moderation form an exchange's own people see on its project pages:
/// take the project out of the exchange's listings with a note, or put it
/// back. Orrery calls this squelching.
#[derive(TemplateOnce)]
#[template(path = "exchange/squelchbutton.stpl.html")]
struct SquelchButton {
    exchange: i64,
    project: String,
    hidden: Option<HiddenProject>,
}

#[derive(Debug, serde::Deserialize)]
struct SquelchForm {
    #[serde(default)]
    elevate: bool,
    #[serde(default)]
    note: String,
}

/// The project a slug names, if any.
async fn project_by_slug(slug: &str) -> Result<Option<Ulid>, Error> {
    let row = sqlx::query_scalar!(
        r#"
        SELECT p.project_id
        FROM projects p
        JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
        WHERE ent.slug = ?
        "#,
        slug
    )
    .fetch_optional(db())
    .await?;

    row.map(|project_id| Ulid::from_string(&project_id))
        .transpose()
        .map_err(Error::from)
}

/// Take a project out of an exchange's listings.
///
/// Squelching one already squelched keeps the note it has, as orrery does.
#[handler]
pub async fn squelch_project(
    Path((path_id, project_slug)): Path<(i64, String)>,
    form: poem::web::Form<SquelchForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let info = ExchangeInfo::load_by_id(path_id).await?;

    if !access::may_manage(info.exchange_id, Some(&*user_info)).await? {
        return Ok(error_response(poem::http::StatusCode::FORBIDDEN, ""));
    }

    let Some(project_id) = project_by_slug(&project_slug).await? else {
        return Ok(error_response(
            poem::http::StatusCode::NOT_FOUND,
            "Project not found",
        ));
    };

    let mut exchange = Exchange::load(info.exchange_id).await?;
    let hidden = exchange
        .hidden_projects
        .entry(project_id)
        .or_insert_with(|| HiddenProject {
            hidden_by: user_info.user_id.to_string(),
            hidden_at: TimeString::now().to_string(),
            note: form.note.trim().to_string(),
            elevate: form.elevate,
        })
        .clone();
    exchange.store().await?;

    Ok(Html(
        SquelchButton {
            exchange: path_id,
            project: project_slug,
            hidden: Some(hidden),
        }
        .render_once()?,
    )
    .into_response())
}

/// Put a squelched project back into an exchange's listings.
#[handler]
pub async fn unsquelch_project(
    Path((path_id, project_slug)): Path<(i64, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let info = ExchangeInfo::load_by_id(path_id).await?;

    if !access::may_manage(info.exchange_id, Some(&*user_info)).await? {
        return Ok(error_response(poem::http::StatusCode::FORBIDDEN, ""));
    }

    let Some(project_id) = project_by_slug(&project_slug).await? else {
        return Ok(error_response(
            poem::http::StatusCode::NOT_FOUND,
            "Project not found",
        ));
    };

    let mut exchange = Exchange::load(info.exchange_id).await?;
    if exchange.hidden_projects.remove(&project_id).is_some() {
        exchange.store().await?;
    }

    Ok(Html(
        SquelchButton {
            exchange: path_id,
            project: project_slug,
            hidden: None,
        }
        .render_once()?,
    )
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::super::ExchangeSettings;
    use super::*;
    use sqlx::SqlitePool;

    const EXCHANGE: &str = "01K00000000000000000000EXC";
    const OTHER_EXCHANGE: &str = "01K00000000000000000000OTH";

    /// One project per case, named for what the listing rule should do with
    /// it, in a schema cut down to the columns an exchange search reads.
    async fn catalogue() -> SqlitePool {
        let pool = SqlitePool::connect(":memory:").await.unwrap();

        for statement in [
            "CREATE TABLE projects (project_id TEXT, name TEXT, image TEXT, image_credit TEXT,
                goal TEXT, task TEXT, exchange TEXT, flagged INTEGER, duplicate INTEGER,
                is_test INTEGER, status TEXT, \"end\" TEXT)",
            "CREATE TABLE entities (target TEXT, kind TEXT, slug TEXT, publish INTEGER)",
            "CREATE TABLE exchanges (exchange_id TEXT, name TEXT)",
            "CREATE TABLE exchange_hidden_projects (exchange_id TEXT, project_id TEXT)",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }

        let projects = [
            // (name, status, end, flagged, duplicate, is_test)
            ("active", "Active", "", false, false, false),
            (
                "active-ended",
                "Active",
                "2000-01-01 00:00:00",
                false,
                false,
                false,
            ),
            ("periodic", "Periodically Active", "", false, false, false),
            ("complete", "Complete", "", false, false, false),
            ("hidden-status", "Hidden", "", false, false, false),
            ("flagged", "Active", "", true, false, false),
            ("duplicate", "Active", "", false, true, false),
            ("test", "Active", "", false, false, true),
            ("hidden-here", "Active", "", false, false, false),
            ("hidden-elsewhere", "Active", "", false, false, false),
        ];

        for (name, status, end, flagged, duplicate, is_test) in projects {
            sqlx::query("INSERT INTO projects VALUES (?, ?, '', '', '', '', NULL, ?, ?, ?, ?, ?)")
                .bind(name)
                .bind(name)
                .bind(flagged)
                .bind(duplicate)
                .bind(is_test)
                .bind(status)
                .bind(end)
                .execute(&pool)
                .await
                .unwrap();

            sqlx::query("INSERT INTO entities VALUES (?, 'Project', ?, 1)")
                .bind(name)
                .bind(name)
                .execute(&pool)
                .await
                .unwrap();
        }

        for (exchange, project) in [
            (EXCHANGE, "hidden-here"),
            (OTHER_EXCHANGE, "hidden-elsewhere"),
        ] {
            sqlx::query("INSERT INTO exchange_hidden_projects VALUES (?, ?)")
                .bind(exchange)
                .bind(project)
                .execute(&pool)
                .await
                .unwrap();
        }

        pool
    }

    /// The names an exchange search lists, built the way the handlers build it.
    async fn listed(pool: &SqlitePool, inactive: bool, include_hidden: bool) -> Vec<String> {
        let scope = SearchScope {
            regions: Vec::new(),
            phrases: Vec::new(),
            include_hidden,
            now: "2026-10-08 00:00:00".into(),
        };
        let params = SearchParams {
            inactive,
            ..Default::default()
        };

        let mut query_builder = sqlx::QueryBuilder::new("");
        build_base_where_clause(
            &mut query_builder,
            Ulid::from_string(EXCHANGE).unwrap(),
            &scope,
            false,
        );
        push_listing_filter(&mut query_builder, &params, &scope.now);
        query_builder.push(" ORDER BY p.name");

        query_builder
            .build_query_as::<(
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                String,
            )>()
            .fetch_all(pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.2)
            .collect()
    }

    #[tokio::test]
    async fn an_orrery_project_address_finds_the_slug() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();

        for statement in [
            "CREATE TABLE projects (project_id TEXT, legacy_v2_project_id INTEGER)",
            "CREATE TABLE entities (target TEXT, kind TEXT, slug TEXT)",
            "INSERT INTO projects VALUES ('01K000000000000000000PROJ', 1214)",
            "INSERT INTO entities VALUES ('01K000000000000000000PROJ', 'Project', 'rubin-comet-catchers')",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }

        assert_eq!(
            orrery_project_slug(&pool, "1214").await.unwrap().as_deref(),
            Some("rubin-comet-catchers")
        );
        assert_eq!(orrery_project_slug(&pool, "429").await.unwrap(), None);
        assert_eq!(
            orrery_project_slug(&pool, "rubin-comet-catchers")
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn default_search_lists_only_running_active_projects() {
        let pool = catalogue().await;

        assert_eq!(
            listed(&pool, false, false).await,
            ["active", "hidden-elsewhere"]
        );
    }

    #[tokio::test]
    async fn inactive_search_never_lists_hidden_flagged_duplicate_or_test() {
        let pool = catalogue().await;

        assert_eq!(
            listed(&pool, true, false).await,
            [
                "active",
                "active-ended",
                "complete",
                "hidden-elsewhere",
                "periodic"
            ]
        );
    }

    #[tokio::test]
    async fn exchange_staff_see_what_their_exchange_hid() {
        let pool = catalogue().await;

        assert_eq!(
            listed(&pool, false, true).await,
            ["active", "hidden-elsewhere", "hidden-here"]
        );
    }

    /// One project per membership case, with REGEXP available the way the
    /// shared pool has it.
    async fn membership_catalogue() -> SqlitePool {
        use sqlx::sqlite::SqliteConnectOptions;
        use std::str::FromStr;

        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .with_regexp();
        let pool = SqlitePool::connect_with(options).await.unwrap();

        for statement in [
            "CREATE TABLE projects (project_id TEXT, name TEXT, exchange TEXT, origin TEXT,
                location_type TEXT)",
            "CREATE TABLE project_tags (project_id TEXT, tag TEXT)",
            "CREATE TABLE project_regions (project_id TEXT, region_id INTEGER)",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }

        let projects = [
            // (name, submitted through, origin, tags)
            ("own", Some(EXCHANGE), "scistarter", vec![]),
            ("own-citsci", Some(EXCHANGE), "CitSci.org", vec![]),
            ("elsewhere", Some(OTHER_EXCHANGE), "scistarter", vec![]),
            ("tagged", None, "scistarter", vec!["NZ"]),
            ("citsci", None, "CitSci.org", vec![]),
            ("fish", None, "scistarter", vec!["fish", "ocean"]),
            ("Birdwatch", None, "scistarter", vec![]),
        ];

        for (name, exchange, origin, tags) in projects {
            sqlx::query("INSERT INTO projects VALUES (?, ?, ?, ?, '')")
                .bind(name)
                .bind(name)
                .bind(exchange)
                .bind(origin)
                .execute(&pool)
                .await
                .unwrap();

            for tag in tags {
                sqlx::query("INSERT INTO project_tags VALUES (?, ?)")
                    .bind(name)
                    .bind(tag)
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        }

        pool
    }

    /// The names an exchange with `settings` is made up of, global projects
    /// included.
    async fn members(pool: &SqlitePool, settings: ExchangeSettings) -> Vec<String> {
        members_with_global(pool, settings, true).await
    }

    /// The names an exchange with `settings` is made up of, with "include
    /// global projects" as given.
    async fn members_with_global(
        pool: &SqlitePool,
        settings: ExchangeSettings,
        include_global: bool,
    ) -> Vec<String> {
        let exchange = ExchangeInfo {
            exchange_id: Ulid::from_string(EXCHANGE).unwrap(),
            path_id: 1,
            slug: "exchange".into(),
            name: "Exchange".into(),
            description: String::new(),
            logo_url: None,
            settings,
        };

        let mut query_builder = sqlx::QueryBuilder::new("SELECT p.name FROM projects p WHERE 1");
        push_membership(&mut query_builder, &exchange, include_global);
        query_builder.push(" ORDER BY p.name");

        query_builder
            .build_query_scalar::<String>()
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn an_exchange_is_the_whole_catalogue_by_default() {
        let pool = membership_catalogue().await;

        assert_eq!(
            members(&pool, ExchangeSettings::default()).await,
            [
                "Birdwatch",
                "citsci",
                "elsewhere",
                "fish",
                "own",
                "own-citsci",
                "tagged"
            ]
        );
    }

    #[tokio::test]
    async fn excluding_scistarter_leaves_its_own_and_what_it_includes() {
        let pool = membership_catalogue().await;

        let settings = ExchangeSettings {
            exclude_scistarter: true,
            include_tags: vec!["nz".into()],
            include_origins: vec!["CitSci.org".into()],
            ..Default::default()
        };

        assert_eq!(
            members(&pool, settings).await,
            ["citsci", "own", "own-citsci", "tagged"]
        );

        let settings = ExchangeSettings {
            exclude_scistarter: true,
            ..Default::default()
        };

        assert_eq!(members(&pool, settings).await, ["own", "own-citsci"]);
    }

    #[tokio::test]
    async fn includes_do_nothing_without_excluding_scistarter() {
        let pool = membership_catalogue().await;

        let settings = ExchangeSettings {
            include_tags: vec!["nz".into()],
            ..Default::default()
        };

        assert_eq!(members(&pool, settings).await.len(), 7);
    }

    #[tokio::test]
    async fn excluded_origins_come_off_its_own_projects_too() {
        let pool = membership_catalogue().await;

        let settings = ExchangeSettings {
            exclude_scistarter: true,
            excluded_origins: vec!["CitSci.org".into()],
            ..Default::default()
        };

        assert_eq!(members(&pool, settings).await, ["own"]);
    }

    #[tokio::test]
    async fn the_search_matches_name_or_tags_ignoring_case() {
        let pool = membership_catalogue().await;

        let settings = ExchangeSettings {
            search: "BIRD|FISH".into(),
            ..Default::default()
        };

        assert_eq!(members(&pool, settings).await, ["Birdwatch", "fish"]);

        // Tags are matched as orrery stores them, so a pattern written
        // against that form behaves the same here.
        let settings = ExchangeSettings {
            search: r"\|\|ocean\|\|".into(),
            ..Default::default()
        };

        assert_eq!(members(&pool, settings).await, ["fish"]);
    }

    #[test]
    fn visits_say_from_when_they_run() {
        let since = Some("2026-10-09T15:00:00Z");

        assert_eq!(
            tracking_note(since, None).as_deref(),
            Some("since October 9, 2026")
        );
        assert_eq!(
            tracking_note(since, Some(2026)).as_deref(),
            Some("since October 9, 2026")
        );
        assert_eq!(
            tracking_note(since, Some(2025)).as_deref(),
            Some("since October 9, 2026")
        );
        assert_eq!(tracking_note(since, Some(2027)), None);
        assert_eq!(
            tracking_note(Some("2026-01-01T09:00:00Z"), Some(2026)),
            None,
            "tracked from the first day of the year asked about"
        );
        assert_eq!(
            tracking_note(None, None).as_deref(),
            Some("not yet tracked")
        );
    }

    #[test]
    fn submission_kinds_round_trip_through_their_urls() {
        for kind in [SubmissionKind::Project, SubmissionKind::Event] {
            assert_eq!(SubmissionKind::from_segment(kind.segment()), Some(kind));
        }
        assert_eq!(SubmissionKind::from_segment("tools"), None);
    }

    #[test]
    fn a_queued_submission_links_to_its_own_exchange_page() {
        let event = PendingSubmission {
            kind: SubmissionKind::Event,
            id: Ulid::from_string(EXCHANGE).unwrap(),
            name: "<b>Bioblitz</b>".into(),
            slug: Some("bioblitz".into()),
        };

        let html = event.to_html(38);

        assert!(html.contains(r#"href="/exchange/38/bioblitz/event.html""#));
        assert!(html.contains(&format!("/exchange/38/events/{EXCHANGE}/approve")));
        assert!(html.contains("&lt;b&gt;Bioblitz&lt;/b&gt;"));
    }

    #[tokio::test]
    async fn being_global_is_enough_only_while_global_projects_are_included() {
        let pool = membership_catalogue().await;

        const REGION: i64 = 7;
        for (name, location_type, in_region) in [
            ("global-elsewhere", GLOBAL_LOCATION_TYPE, false),
            ("global-here", GLOBAL_LOCATION_TYPE, true),
            ("local", "At a specific address", true),
        ] {
            sqlx::query("INSERT INTO projects VALUES (?, ?, NULL, 'scistarter', ?)")
                .bind(name)
                .bind(name)
                .bind(location_type)
                .execute(&pool)
                .await
                .unwrap();

            if in_region {
                sqlx::query("INSERT INTO project_regions VALUES (?, ?)")
                    .bind(name)
                    .bind(REGION)
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        }

        let regional = || ExchangeSettings {
            region_id: Some(REGION),
            ..Default::default()
        };

        assert_eq!(
            members_with_global(&pool, regional(), true).await,
            ["global-elsewhere", "global-here", "local"]
        );
        assert_eq!(
            members_with_global(&pool, regional(), false).await,
            ["global-here", "local"]
        );
    }

    #[test]
    fn the_initial_search_takes_the_exchanges_choice() {
        let mut exchange = ExchangeInfo {
            exchange_id: Ulid::from_string(EXCHANGE).unwrap(),
            path_id: 1,
            slug: "exchange".into(),
            name: "Exchange".into(),
            description: String::new(),
            logo_url: None,
            settings: ExchangeSettings::default(),
        };
        assert!(
            exchange.initial_search().global,
            "on unless chosen otherwise"
        );

        exchange.settings.global_by_default = false;
        assert!(!exchange.initial_search().global);
    }

    #[test]
    fn a_search_that_will_not_compile_is_refused() {
        assert!(search_error("bird|fish").is_none());
        assert!(search_error("bird(").is_some());
    }
}
