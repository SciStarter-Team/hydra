use anyhow::Error;
use common::{
    message::{
        log::{ExchangeEvent, ProjectLikesResponse, RatingSetRequest},
        user::{PermissionCheckRequest, PermissionCheckResponse},
    },
    quill::content_to_html,
    service::{request, send},
    shared::db,
    timestring::TimeString,
};
use poem::{
    IntoResponse, handler,
    session::Session,
    web::{Html, Path, Query, Redirect},
};
use sailfish::TemplateOnce;
use std::sync::OnceLock;
use ulid::Ulid;

use crate::local_time::{TimeFormat, local_time};
use crate::middleware::analytics::{Device, Interaction};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;

use super::{
    Exchange, ExchangeInfo, ExchangeSettings, SearchParams, TemplateExchange, TemplateUser,
};
use crate::project::location::Where;

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
    exchange_id: i64,
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
    query_builder.push_bind(exchange_id);
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
    exchange_id: i64,
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
            AND p.flagged = false
            AND p.duplicate = false
            AND p.is_test = false
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
            AND p.flagged = false
            AND p.duplicate = false
            AND p.is_test = false
            "#,
        );

        push_hidden_exclusion(query_builder, exchange_id, scope);
    }
}

/// The `location_type` a project that can be done anywhere carries.
///
/// Spelled out rather than coded, because that is how orrery's sync sends it
/// and therefore how hydra stores it.
const GLOBAL_LOCATION_TYPE: &str = "Global, anywhere on the planet";

/// The `status` a project that is currently running carries.
const ACTIVE_STATUS: &str = "Active";

/// Narrow a query to the projects this exchange is a view of.
///
/// An exchange with a region shows the projects that reach it plus the ones
/// that can be done anywhere, which is how "available in Australia" is
/// expressed; an exchange without one is a view of the whole catalogue.
/// Excluded origins come off the top either way.
///
/// The region test reads the cached matches rather than measuring polygons
/// per query, so it agrees with the region tick-boxes by construction.
///
/// Orrery has further levers here that this does not, deferred rather than
/// dropped. As of Aug 2026 only the `Test` exchange uses any of them, which
/// is why they are not here yet -- not because they are unwanted.
///
/// They do not all stand alone. `exclude_scistarter` is not a fourth filter
/// alongside the rest: it inverts the rule, turning an exchange from "the
/// catalogue, narrowed" into "nothing, plus whatever its include-tags and
/// include-origins name". Those two include levers do nothing at all without
/// it, and orrery has a standing demonstration of that -- SciStarter
/// Australia carries an `australia` include-tag that has never had any
/// effect, because its `exclude_scistarter` is false. So port
/// `exclude_scistarter` together with the include levers, or the tag lever
/// will arrive looking broken. A `search` regex over name and tags is
/// independent of all three and can come on its own.
///
/// Whichever arrives, it means a field on [`ExchangeSettings`] and a clause
/// here; the settings are stored as JSON with serde defaults, so it costs no
/// migration and leaves existing exchanges reading as they do now.
fn push_membership(
    query_builder: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    settings: &ExchangeSettings,
) {
    if let Some(region_id) = settings.region_id {
        query_builder.push(
            " AND (p.project_id IN (SELECT project_id FROM project_regions WHERE region_id = ",
        );
        query_builder.push_bind(region_id);
        query_builder.push(") OR p.location_type = ");
        query_builder.push_bind(GLOBAL_LOCATION_TYPE);
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
}

/// Leave out the projects that are no longer running, unless asked not to.
///
/// "Inactive" covers both a status that is not Active and an end date that
/// has passed, which are different ways for a project to have finished.
fn push_status(
    query_builder: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    params: &SearchParams,
    now: &str,
) {
    if params.inactive {
        return;
    }

    query_builder.push(" AND p.status = ");
    query_builder.push_bind(ACTIVE_STATUS);
    query_builder.push(" AND (p.\"end\" IS NULL OR p.\"end\" = '' OR p.\"end\" > ");
    query_builder.push_bind(now.to_owned());
    query_builder.push(")");
}

/// Apply common search filters to a query builder
fn apply_search_filters<'a>(
    query_builder: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    exchange: &'a ExchangeInfo,
    params: &'a SearchParams,
    scope: &'a SearchScope,
) {
    push_membership(query_builder, &exchange.settings);
    push_status(query_builder, params, &scope.now);

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

/// Check if the current user is authorized to manage this exchange
async fn check_user_authorization(exchange_id: i64, auth: &Authenticated) -> Result<bool, Error> {
    if let Some(ref user_info) = auth.0 {
        // Staff and superusers have full access to all exchanges
        if user_info.is_staff || user_info.is_superuser {
            return Ok(true);
        }

        // Check permission via user-service for regular users
        let response = request::<PermissionCheckResponse>(
            "user.permissions.check",
            PermissionCheckRequest {
                user_id: user_info.user_id.to_string(),
                resource_type: "exchange".to_string(),
                resource_id: exchange_id.to_string(),
            },
        )
        .await?;

        Ok(response.has_permission)
    } else {
        Ok(false)
    }
}

/// Create a TemplateExchange with proper authorization checking
async fn create_template_exchange(
    exchange_info: ExchangeInfo,
    auth: &Authenticated,
) -> Result<TemplateExchange, Error> {
    let user_is_authorized = check_user_authorization(exchange_info.path_id, auth).await?;

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
}

impl ExchangeProjectPage {
    fn encode(&self, s: &str) -> String {
        urlencoding::encode(s).to_string()
    }
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
        // Check authorization
        let has_permission = request::<PermissionCheckResponse>(
            "user.permissions.check",
            PermissionCheckRequest {
                user_id: user.user_id.to_string(),
                resource_type: "exchange".to_string(),
                resource_id: exchange_info.exchange_id.to_string(),
            },
        )
        .await?
        .has_permission;

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

    // Track visit
    if let Some(ref user_info) = auth.0 {
        record_event(
            exchange.info.exchange_id,
            Some(user_info.user_id),
            &device,
            &interaction,
            "home",
            None,
            "view",
        )
        .await?;
    }

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

    // Load initial project results (first page, no filters)
    let params = SearchParams::default();
    let scope = SearchScope::resolve(&params, exchange.user_is_authorized).await?;
    let page = 1;
    let page_size = 20;
    let offset = 0;

    // Build query for initial projects
    let mut query_builder = sqlx::QueryBuilder::new("");
    build_base_where_clause(&mut query_builder, exchange.info.path_id, &scope, false);
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
    build_base_where_clause(&mut count_builder, exchange.info.path_id, &scope, true);
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

    // Track search
    if let Some(ref user_info) = auth.0 {
        record_event(
            exchange.info.exchange_id,
            Some(user_info.user_id),
            &device,
            &interaction,
            "results",
            None,
            "search",
        )
        .await?;
    }

    // Get pagination from params
    let page = params.page.max(1);
    let page_size = params.page_size.max(1).min(100);
    let offset = (page - 1) * page_size;

    // Build base query using QueryBuilder
    let scope = SearchScope::resolve(&params, exchange.user_is_authorized).await?;
    let has_text_search = scope.has_text_search();
    let mut query_builder = sqlx::QueryBuilder::new("");

    // Build base WHERE clause
    build_base_where_clause(&mut query_builder, exchange.info.path_id, &scope, false);

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
    build_base_where_clause(&mut count_builder, exchange.info.path_id, &scope, true);

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

/// Exchange CSV export (requires authorization)
#[handler]
pub async fn exchange_csv(
    Path(exchange_id): Path<i64>,
    crate::form::QsQuery(params): crate::form::QsQuery<SearchParams>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let exchange = ExchangeInfo::load_by_id(exchange_id).await?;

    // Check permission - staff/superuser OR exchange-specific permission
    let has_permission = if user_info.is_staff || user_info.is_superuser {
        true
    } else {
        request::<PermissionCheckResponse>(
            "user.permissions.check",
            PermissionCheckRequest {
                user_id: user_info.user_id.to_string(),
                resource_type: "exchange".to_string(),
                resource_id: exchange.exchange_id.to_string(),
            },
        )
        .await?
        .has_permission
    };

    if !has_permission {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::FORBIDDEN)
            .body("Forbidden"));
    }

    // Only the exchange's own people reach this export, so it carries the
    // hidden projects along with the rest.
    let scope = SearchScope::resolve(&params, true).await?;

    // Build query for ALL projects (no pagination) with same filters as search
    let has_text_search = scope.has_text_search();
    let mut query_builder = sqlx::QueryBuilder::new("");

    // Build base WHERE clause (no pagination - get all results)
    build_base_where_clause(&mut query_builder, exchange.path_id, &scope, false);
    apply_search_filters(&mut query_builder, &exchange, &params, &scope);

    // Add ordering (same as search results)
    if has_text_search {
        query_builder.push(" ORDER BY bm25(projects_fts), p.created DESC");
    } else {
        query_builder.push(" ORDER BY p.created DESC");
    }

    // Execute query (no LIMIT - get all results for CSV)
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

    // Build CSV content
    let mut csv_content = String::new();
    if has_permission {
        csv_content
            .push_str("project_id,slug,name,image,image_credit,goal,task,exchange,admin_link\n");
    } else {
        csv_content.push_str("project_id,slug,name,image,image_credit,goal,task,exchange\n");
    }

    for (project_id, slug, name, image, image_credit, goal, task, exchange, admin_link) in rows {
        // Escape CSV fields (basic escaping - wrap in quotes if contains comma or quote)
        let escape_csv = |s: &str| -> String {
            if s.contains(',') || s.contains('"') || s.contains('\n') {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_string()
            }
        };

        if has_permission {
            csv_content.push_str(&format!(
                "{},{},{},{},{},{},{},{},{}\n",
                escape_csv(&project_id),
                escape_csv(&slug),
                escape_csv(&name),
                escape_csv(&image),
                escape_csv(&image_credit),
                escape_csv(&goal),
                escape_csv(&task),
                escape_csv(&exchange),
                escape_csv(&admin_link)
            ));
        } else {
            csv_content.push_str(&format!(
                "{},{},{},{},{},{},{},{}\n",
                escape_csv(&project_id),
                escape_csv(&slug),
                escape_csv(&name),
                escape_csv(&image),
                escape_csv(&image_credit),
                escape_csv(&goal),
                escape_csv(&task),
                escape_csv(&exchange)
            ));
        }
    }

    Ok(poem::Response::builder()
        .header("Content-Type", "text/csv")
        .header(
            "Content-Disposition",
            "attachment; filename=\"projects.csv\"",
        )
        .body(csv_content))
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
    pending: Vec<PendingProject>,
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

#[derive(Clone)]
struct PendingProject {
    project_id: Ulid,
    name: String,
    slug: Option<String>,
}

impl PendingProject {
    fn to_html(&self, exchange_id: i64) -> String {
        let name = self
            .name
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");

        // Build project URL - use slug if available, otherwise use project_id
        let project_url = if let Some(slug) = &self.slug {
            format!("/exchange/{}/{}/project.html", exchange_id, slug)
        } else {
            format!("/exchange/{}/{}/project.html", exchange_id, self.project_id)
        };

        format!(
            r#"<div class="org-item oi-line" style="gap: 8px;">
                <span class="label"><a href="{}" target="_blank" style="color: inherit; text-decoration: underline;">{}</a></span>
                <div style="display: flex; gap: 8px;">
                    <button type="button" style="width: auto; min-width: 80px; height: 32px; padding: 0 12px; line-height: 1;" hx-post="/exchange/{}/projects/{}/approve" hx-target="closest .org-item" hx-swap="outerHTML" class="el-button el-button--primary">Approve</button>
                    <button type="button" style="width: auto; min-width: 80px; height: 32px; padding: 0 12px; line-height: 1;" hx-delete="/exchange/{}/projects/{}/reject" hx-target="closest .org-item" hx-swap="outerHTML" class="el-button el-button--danger">Reject</button>
                </div>
            </div>"#,
            project_url, name, exchange_id, self.project_id, exchange_id, self.project_id
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
) -> Result<poem::Response, Error> {
    let exchange_info = ExchangeInfo::load_by_id(exchange_id).await?;
    let exchange = create_template_exchange(exchange_info, &auth).await?;

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
        },
    )
    .await?;

    // Get project details for top projects
    let top_views = fetch_project_details(&response.top_views).await?;
    let top_saves = fetch_project_details(&response.top_saves).await?;

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
        clickthroughs: i32,
        contributions: i32,
        top_views: Vec<(String, String, String, i32)>,
        top_saves: Vec<(String, String, String, i32)>,
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
        clickthroughs: response.clickthroughs,
        contributions: response.contributions,
        top_views,
        top_saves,
    };

    Ok(Html(content.render_once()?).into_response())
}

async fn fetch_project_details(
    project_counts: &[common::message::log::ProjectCount],
) -> Result<Vec<(String, String, String, i32)>, Error> {
    use sqlx::Row;

    let mut results = Vec::new();

    for pc in project_counts {
        // Query project details from database
        let row = sqlx::query(
            r#"
            SELECT name, description, image
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
            let description: String = row.try_get("description").unwrap_or_default();
            let image: String = row.try_get("image").unwrap_or_default();

            results.push((name, description, image, pc.count));
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
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let exchange = ExchangeInfo::load_by_id(exchange_id).await?;

    // Check permission - staff/superuser OR exchange-specific permission
    let has_permission = if user_info.is_staff || user_info.is_superuser {
        true
    } else {
        request::<PermissionCheckResponse>(
            "user.permissions.check",
            PermissionCheckRequest {
                user_id: user_info.user_id.to_string(),
                resource_type: "exchange".to_string(),
                resource_id: exchange.exchange_id.to_string(),
            },
        )
        .await?
        .has_permission
    };

    if !has_permission {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::FORBIDDEN)
            .body("Forbidden"));
    }

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

    // Load existing staff from permissions
    let permissions_response = request::<common::message::user::PermissionListByResourceResponse>(
        "user.permissions.list_by_resource",
        common::message::user::PermissionListByResourceRequest {
            resource_type: "exchange".to_string(),
            resource_id: exchange_id.to_string(),
        },
    )
    .await?;

    // Extract user IDs and batch fetch user info
    let user_ids: Vec<Ulid> = permissions_response
        .permissions
        .iter()
        .filter_map(|perm| Ulid::from_string(&perm.user_id).ok())
        .collect();

    let mut staff = Vec::new();
    if !user_ids.is_empty() {
        match request::<common::message::user::UserDetailsBatchGetResponse>(
            "user.admin.user_batch_get",
            common::message::user::UserDetailsBatchGetRequest {
                user_ids: user_ids.clone(),
            },
        )
        .await
        {
            Ok(user_batch_response) => {
                for user_mapping in user_batch_response.users {
                    staff.push(StaffMember {
                        user_id: user_mapping.user_id,
                        name: user_mapping.primary_email.clone(), // Use email as name for now
                        email: user_mapping.primary_email,
                    });
                }
            }
            Err(_e) => {
                // Failed to fetch user details, but continue
            }
        }
    }

    // Load pending projects for approval
    // Projects that need approval are those with exchange set and reviewed = 0
    let exchange_id_str = exchange.exchange_id.to_string();
    let pending_rows = sqlx::query!(
        r#"
        SELECT p.project_id, p.name, e.slug
        FROM projects p
        LEFT JOIN entities e ON e.kind = 'Project' AND e.target = p.project_id
        WHERE p.exchange = ?
          AND p.reviewed = 0
        ORDER BY p.name
        "#,
        exchange_id_str
    )
    .fetch_all(db())
    .await?;

    let pending: Vec<PendingProject> = pending_rows
        .into_iter()
        .filter_map(|row| {
            Ulid::from_string(&row.project_id)
                .ok()
                .map(|project_id| PendingProject {
                    project_id,
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
        owner: None, // TODO: determine from exchange ownership
        staff,
        pending,
    };

    Ok(Html(page.render_once()?).into_response())
}

#[derive(Debug, serde::Deserialize)]
struct ExchangeSettingsForm {
    #[serde(default)]
    allow_project_voting: bool,
    #[serde(default)]
    allow_project_reviews: bool,
    #[serde(default)]
    require_approval: bool,
    #[serde(default)]
    restricted_add: bool,
    #[serde(default)]
    custom_css: String,
    /// Empty means "no region": a view of the whole catalogue.
    #[serde(default)]
    region_id: String,
}

/// Update exchange settings (uses LoadStorePeered for write)
#[handler]
pub async fn exchange_update_settings(
    Path(path_id): Path<i64>,
    form: poem::web::Form<ExchangeSettingsForm>,
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

    // Load full Exchange via LoadStorePeered for modification
    let mut exchange = Exchange::load(exchange_id).await?;

    // Check permission - staff/superuser OR exchange-specific permission
    let has_permission = if user_info.is_staff || user_info.is_superuser {
        true
    } else {
        request::<PermissionCheckResponse>(
            "user.permissions.check",
            PermissionCheckRequest {
                user_id: user_info.user_id.to_string(),
                resource_type: "exchange".to_string(),
                resource_id: exchange.exchange_id.to_string(),
            },
        )
        .await?
        .has_permission
    };

    if !has_permission {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::FORBIDDEN)
            .body("Forbidden"));
    }

    // Update settings from form data
    exchange.settings.allow_project_voting = form.allow_project_voting;
    exchange.settings.allow_project_reviews = form.allow_project_reviews;
    exchange.settings.require_approval = form.require_approval;
    exchange.settings.restricted_add = form.restricted_add;
    exchange.settings.region_id = form.region_id.trim().parse().ok();
    exchange.settings.custom_css = if form.custom_css.is_empty() {
        None
    } else {
        Some(form.custom_css.clone())
    };

    // Use LoadStorePeered to save and broadcast to peers
    exchange.store().await?;

    Ok(Redirect::see_other(format!("/exchange/{}/org.html", exchange.exchange_id)).into_response())
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

    // Check permission - staff/superuser OR exchange-specific permission
    let has_permission = if user_info.is_staff || user_info.is_superuser {
        true
    } else {
        request::<PermissionCheckResponse>(
            "user.permissions.check",
            PermissionCheckRequest {
                user_id: user_info.user_id.to_string(),
                resource_type: "exchange".to_string(),
                resource_id: exchange_id.to_string(),
            },
        )
        .await?
        .has_permission
    };

    if !has_permission {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::FORBIDDEN)
            .body("Forbidden"));
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

    // Check permission - staff/superuser OR exchange-specific permission
    let has_permission = if user_info.is_staff || user_info.is_superuser {
        true
    } else {
        request::<PermissionCheckResponse>(
            "user.permissions.check",
            PermissionCheckRequest {
                user_id: user_info.user_id.to_string(),
                resource_type: "exchange".to_string(),
                resource_id: exchange.exchange_id.to_string(),
            },
        )
        .await?
        .has_permission
    };

    if !has_permission {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::FORBIDDEN)
            .body("Forbidden"));
    }

    // Parse form data and create predefined search
    let phrases: Vec<String> = form
        .phrase
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

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
        search_name.replace('"', "&quot;"),
        phrase.replace('"', "&quot;"),
        exchange_id,
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

    // Check permission - staff/superuser OR exchange-specific permission
    let has_permission = if user_info.is_staff || user_info.is_superuser {
        true
    } else {
        request::<PermissionCheckResponse>(
            "user.permissions.check",
            PermissionCheckRequest {
                user_id: user_info.user_id.to_string(),
                resource_type: "exchange".to_string(),
                resource_id: exchange.exchange_id.to_string(),
            },
        )
        .await?
        .has_permission
    };

    if !has_permission {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::FORBIDDEN)
            .body("Forbidden"));
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

    // Check permission - staff/superuser OR exchange-specific permission
    let has_permission = if user_info.is_staff || user_info.is_superuser {
        true
    } else {
        request::<PermissionCheckResponse>(
            "user.permissions.check",
            PermissionCheckRequest {
                user_id: user_info.user_id.to_string(),
                resource_type: "exchange".to_string(),
                resource_id: exchange_id.to_string(),
            },
        )
        .await?
        .has_permission
    };

    if !has_permission {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::FORBIDDEN)
            .body("Forbidden"));
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
                    // Grant permission
                    match request::<()>(
                        "user.permissions.grant",
                        common::message::user::PermissionGrantRequest {
                            user_id: user_id_str.clone(),
                            resource_type: "exchange".to_string(),
                            resource_id: exchange_id.to_string(),
                            permission: "manage".to_string(),
                            granted_by: user_info.user_id.to_string(),
                        },
                    )
                    .await
                    {
                        Ok(_) => {}
                        Err(_e) => {
                            continue; // Skip adding to the list if grant failed
                        }
                    }

                    // Parse user_id as ULID for display
                    if let Ok(user_ulid) = Ulid::from_string(&user_id_str) {
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

    // Check permission - staff/superuser OR exchange-specific permission
    let has_permission = if user_info.is_staff || user_info.is_superuser {
        true
    } else {
        request::<PermissionCheckResponse>(
            "user.permissions.check",
            PermissionCheckRequest {
                user_id: user_info.user_id.to_string(),
                resource_type: "exchange".to_string(),
                resource_id: exchange_id.to_string(),
            },
        )
        .await?
        .has_permission
    };

    if !has_permission {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::FORBIDDEN)
            .body("Forbidden"));
    }

    // Revoke permission
    let _ = request::<()>(
        "user.permissions.revoke",
        common::message::user::PermissionRevokeRequest {
            user_id: user_id_str,
            resource_type: "exchange".to_string(),
            resource_id: exchange_id.to_string(),
        },
    )
    .await;

    // Return empty response (HTMX will remove the element)
    Ok(Html("").into_response())
}

/// Approve a project for an exchange
#[handler]
pub async fn approve_exchange_project(
    Path((exchange_id, project_id_str)): Path<(i64, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    // Check permission - staff/superuser OR exchange-specific permission
    let has_permission = if user_info.is_staff || user_info.is_superuser {
        true
    } else {
        request::<PermissionCheckResponse>(
            "user.permissions.check",
            PermissionCheckRequest {
                user_id: user_info.user_id.to_string(),
                resource_type: "exchange".to_string(),
                resource_id: exchange_id.to_string(),
            },
        )
        .await?
        .has_permission
    };

    if !has_permission {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::FORBIDDEN)
            .body("Forbidden"));
    }

    let project_id = Ulid::from_string(&project_id_str)?;

    // Verify the project belongs to this exchange
    let project_exchange = sqlx::query_scalar!(
        r#"SELECT exchange FROM projects WHERE project_id = ?"#,
        project_id_str
    )
    .fetch_optional(db())
    .await?;

    match project_exchange {
        Some(Some(exchange_str)) if exchange_str == exchange_id.to_string() => {
            // Project belongs to this exchange, proceed with approval
        }
        _ => {
            return Ok(poem::Response::builder()
                .status(poem::http::StatusCode::BAD_REQUEST)
                .body("Project does not belong to this exchange"));
        }
    }

    // Load project name for slug generation
    let project_name = sqlx::query_scalar!(
        r#"SELECT name FROM projects WHERE project_id = ?"#,
        project_id_str
    )
    .fetch_one(db())
    .await?;

    // Check if entity already exists
    let existing_entity = sqlx::query!(
        r#"SELECT slug, created FROM entities WHERE kind = 'Project' AND target = ?"#,
        project_id_str
    )
    .fetch_optional(db())
    .await?;

    let entity = if let Some(existing) = existing_entity {
        // Update existing entity to publish
        use crate::entity::{Entity, EntityKind, EntityTarget};
        Entity::new_with_created(
            existing.slug,
            EntityKind::Project,
            EntityTarget::Ulid(project_id),
            true, // publish = true
            TimeString::try_from(existing.created)?,
        )
    } else {
        // Create new entity
        use crate::entity::{Entity, EntityKind, EntityTarget};
        Entity::new_by_name(
            &project_name,
            EntityKind::Project,
            EntityTarget::Ulid(project_id),
        )
        .await?
    };

    // Store the entity (this will create or update it with publish = true)
    use crate::peering::LoadStorePeered;
    entity.store().await?;

    // Mark project as reviewed
    use crate::project::field::{ProjectField, ProjectFieldValue};
    let reviewed_field = ProjectField::new(project_id, ProjectFieldValue::Reviewed(true));
    reviewed_field.store().await?;

    tracing::info!(%project_id, exchange_id, "Approved project for exchange");

    // Log the approval action
    let _ = common::service::log_activity(
        None,
        "exchange-project-approve",
        user_info.user_id,
        Some(project_id),
        format!("exchange={}", exchange_id),
    )
    .await;

    // Return empty response (HTMX will remove the element)
    Ok(Html("").into_response())
}

/// Reject a project for an exchange
#[handler]
pub async fn reject_exchange_project(
    Path((exchange_id, project_id_str)): Path<(i64, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    // Check permission - staff/superuser OR exchange-specific permission
    let has_permission = if user_info.is_staff || user_info.is_superuser {
        true
    } else {
        request::<PermissionCheckResponse>(
            "user.permissions.check",
            PermissionCheckRequest {
                user_id: user_info.user_id.to_string(),
                resource_type: "exchange".to_string(),
                resource_id: exchange_id.to_string(),
            },
        )
        .await?
        .has_permission
    };

    if !has_permission {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::FORBIDDEN)
            .body("Forbidden"));
    }

    let project_id = Ulid::from_string(&project_id_str)?;

    // Verify the project belongs to this exchange
    let project_exchange = sqlx::query_scalar!(
        r#"SELECT exchange FROM projects WHERE project_id = ?"#,
        project_id_str
    )
    .fetch_optional(db())
    .await?;

    match project_exchange {
        Some(Some(exchange_str)) if exchange_str == exchange_id.to_string() => {
            // Project belongs to this exchange, proceed with rejection
        }
        _ => {
            return Ok(poem::Response::builder()
                .status(poem::http::StatusCode::BAD_REQUEST)
                .body("Project does not belong to this exchange"));
        }
    }

    // Mark project as reviewed without publishing
    // Project stays associated with exchange but won't appear in pending list
    use crate::project::field::{ProjectField, ProjectFieldValue};
    let reviewed_field = ProjectField::new(project_id, ProjectFieldValue::Reviewed(true));
    reviewed_field.store().await?;

    tracing::info!(%project_id, exchange_id, "Rejected project for exchange (kept association)");

    // Log the rejection action
    let _ = common::service::log_activity(
        None,
        "exchange-project-reject",
        user_info.user_id,
        Some(project_id),
        format!("exchange={}", exchange_id),
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
    .await?
    .ok_or_else(|| anyhow::anyhow!("Project not found"))?;

    // If project is not published, check if user is authorized for the exchange
    if !project_row.publish && !exchange.user_is_authorized {
        return Err(anyhow::anyhow!("Project not found"));
    }

    let project_id = project_row.project_id;

    // Track view
    if let Some(ref user_info) = auth.0 {
        record_event(
            exchange.info.exchange_id,
            Some(user_info.user_id),
            &device,
            &interaction,
            "project",
            Some(project_slug.clone()),
            "view",
        )
        .await?;
    }

    // Load full project data
    let project =
        load_exchange_project_detail(&project_id, &project_slug, auth.0.as_ref(), &exchange_info)
            .await?;

    // Get CSRF token
    let csrftoken = session
        .get::<String>("csrf-token")
        .unwrap_or_else(|| "".to_string());

    let user = TemplateUser::from(auth);

    let template = ExchangeProjectPage {
        exchange,
        user,
        project,
        csrftoken,
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

    // Look up project_id by slug
    let project_id_row = sqlx::query!(
        r#"
        SELECT p.project_id
        FROM projects p
        LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
        WHERE ent.slug = ?
        "#,
        project_slug
    )
    .fetch_optional(common::shared::db())
    .await?;

    let project_id = project_id_row
        .ok_or_else(|| anyhow::anyhow!("Project not found"))?
        .project_id;

    // Save bookmark via user-service
    let response: BookmarkCreateResponse = request(
        "user.bookmarks.create",
        BookmarkCreateRequest {
            user_id: user_info.user_id,
            project_id,
        },
    )
    .await?;

    // Track save action
    record_event(
        exchange.exchange_id,
        Some(user_info.user_id),
        &device,
        &interaction,
        "project",
        Some(project_slug),
        "save",
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

    // Look up project_id by slug
    let project_id_row = sqlx::query!(
        r#"
        SELECT p.project_id
        FROM projects p
        LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
        WHERE ent.slug = ?
        "#,
        project_slug
    )
    .fetch_optional(common::shared::db())
    .await?;

    let project_id_str = project_id_row
        .ok_or_else(|| anyhow::anyhow!("Project not found"))?
        .project_id;

    // Record the like in the log-service (single source of truth for likes)
    let _: ProjectLikesResponse = request(
        "log.rating_set",
        RatingSetRequest {
            project_id: project_id_str,
            user_id: user_info.user_id.to_string(),
            liked: true,
        },
    )
    .await?;

    // Track vote action
    record_event(
        exchange.exchange_id,
        Some(user_info.user_id),
        &device,
        &interaction,
        "project",
        Some(project_slug),
        "vote",
    )
    .await?;

    Ok(Html("Voted").into_response())
}

/// Hide project from exchange (uses LoadStorePeered for write)
#[handler]
pub async fn hide_project(
    Path((path_id, project_slug)): Path<(i64, String)>,
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

    // Check permission
    let has_permission = request::<PermissionCheckResponse>(
        "user.permissions.check",
        PermissionCheckRequest {
            user_id: user_info.user_id.to_string(),
            resource_type: "exchange".to_string(),
            resource_id: exchange.exchange_id.to_string(),
        },
    )
    .await?
    .has_permission;

    if !has_permission {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::FORBIDDEN)
            .body("Forbidden"));
    }

    // Look up project_id by slug
    let project_id_row = sqlx::query!(
        r#"
        SELECT p.project_id
        FROM projects p
        LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
        WHERE ent.slug = ?
        "#,
        project_slug
    )
    .fetch_optional(common::shared::db())
    .await?;

    if let Some(row) = project_id_row {
        let project_id = Ulid::from_string(&row.project_id)?;
        exchange.hidden_projects.insert(project_id);
        exchange.store().await?;

        Ok(Html("Project hidden").into_response())
    } else {
        Ok(poem::Response::builder()
            .status(poem::http::StatusCode::NOT_FOUND)
            .body("Project not found"))
    }
}

/// Track analytics event
async fn record_event(
    exchange_id: Ulid,
    user_id: Option<Ulid>,
    device: impl AsRef<str>,
    interaction: impl AsRef<str>,
    page: &str,
    page_id: Option<String>,
    action: &str,
) -> Result<(), Error> {
    send(
        "log.exchange_event",
        ExchangeEvent {
            event_id: Ulid::new().to_string(),
            exchange_id: exchange_id.to_string(),
            user_id: user_id.map(|u| u.to_string()),
            device_id: device.as_ref().to_string(),
            interaction_id: interaction.as_ref().to_string(),
            when_occurred: TimeString::now().to_string(),
            page: page.to_string(),
            page_id,
            action: action.to_string(),
            extra: None,
        },
    )
    .await?;

    Ok(())
}
