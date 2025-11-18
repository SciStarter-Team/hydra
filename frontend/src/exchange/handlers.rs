use anyhow::Error;
use common::{
    message::{
        log::ExchangeEvent,
        user::{PermissionCheckRequest, PermissionCheckResponse},
    },
    service::{request, send},
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

use crate::middleware::analytics::{Device, Interaction};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::review::ProjectReview;

use super::{Exchange, ExchangeInfo, SearchParams, TemplateExchange, TemplateUser};

// Cache for filter options (loaded once on first use)
static FILTER_CACHE: OnceLock<FilterOptions> = OnceLock::new();

#[derive(Clone)]
struct FilterOptions {
    activities: Vec<(String, String)>,
    topics: Vec<(String, String)>,
    audiences: Vec<(String, String)>,
}

async fn load_filter_options() -> Result<FilterOptions, Error> {
    let activity_rows = sqlx::query!("SELECT DISTINCT activity FROM project_activities ORDER BY activity")
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

    let audience_rows = sqlx::query!("SELECT DISTINCT audience FROM project_audiences ORDER BY audience")
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
fn build_base_where_clause<'a>(
    query_builder: &mut sqlx::QueryBuilder<'a, sqlx::Sqlite>,
    exchange_id: i64,
    params: &'a SearchParams,
    for_count: bool,
) {
    let has_text_search = !params.phrase.is_empty() && params.phrase.iter().any(|p| !p.is_empty());

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
                    p.task
                FROM projects p"#
            );
        }

        query_builder.push(
            r#"
            JOIN projects_fts f ON p.project_id = f.project_id
            LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
            WHERE projects_fts MATCH "#
        );

        // Combine phrases with OR for FTS5
        let search_query = params.phrase.iter()
            .filter(|p| !p.is_empty())
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
            AND p.project_id NOT IN (
                SELECT project_id FROM exchange_hidden_projects
                WHERE exchange_id = "#
        );
        query_builder.push_bind(exchange_id);
        query_builder.push(")");
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
                    p.task
                FROM projects p"#
            );
        }

        query_builder.push(
            r#"
            LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
            WHERE ent.publish = 1
            AND p.flagged = false
            AND p.duplicate = false
            AND p.is_test = false
            AND p.project_id NOT IN (
                SELECT project_id FROM exchange_hidden_projects
                WHERE exchange_id = "#
        );
        query_builder.push_bind(exchange_id);
        query_builder.push(")");
    }
}

/// Apply common search filters to a query builder
fn apply_search_filters<'a>(
    query_builder: &mut sqlx::QueryBuilder<'a, sqlx::Sqlite>,
    _exchange_id: i64,
    params: &'a SearchParams,
) {
    // Add activity filter
    if let Some(ref activity) = params.activity {
        if !activity.is_empty() {
            query_builder.push(" AND p.project_id IN (SELECT project_id FROM project_activities WHERE activity = ");
            query_builder.push_bind(activity);
            query_builder.push(")");
        }
    }

    // Add topic filter
    if let Some(ref topic) = params.topic {
        if !topic.is_empty() {
            query_builder.push(" AND p.project_id IN (SELECT project_id FROM project_topics WHERE topic = ");
            query_builder.push_bind(topic);
            query_builder.push(")");
        }
    }

    // Add audience filters
    if !params.audience.is_empty() {
        query_builder.push(" AND p.project_id IN (SELECT project_id FROM project_audiences WHERE audience IN (");
        let mut separated = query_builder.separated(", ");
        for audience in &params.audience {
            separated.push_bind(audience);
        }
        query_builder.push("))");
    }
}

/// Check if the current user is authorized to manage this exchange
async fn check_user_authorization(
    exchange_id: i64,
    auth: &Authenticated,
) -> Result<bool, Error> {
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
    let user_is_authorized = check_user_authorization(exchange_info.exchange_id, auth).await?;

    Ok(TemplateExchange {
        info: exchange_info,
        user_is_authorized,
    })
}

// Data structures for templates

#[derive(Debug, Clone)]
pub struct ProjectSummary {
    pub slug: String,
    pub name: String,
    pub image: String,
    pub image_credit: String,
    pub goal: String,
    pub task: String,
}

/// Full project detail for exchange project pages
#[derive(Debug, Clone)]
pub struct ExchangeProjectDetail {
    pub id: String,  // slug for URL purposes
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

    // User-specific data
    pub user_is_authorized: bool,
    pub liked: bool,
    pub likes: i64,
    pub saved: bool,
    pub joined: bool,

    pub reviews: Vec<ReviewForDisplay>,
}

/// Review enriched with user display information for template rendering
#[derive(Debug, Clone)]
pub struct ReviewForDisplay {
    pub user_id: String,
    pub user_handle: String,
    pub user_image: String,
    pub text: String,
    pub when: TimeString,
}

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
                parts.push(format!("phrase={}", urlencoding::encode(phrase)));
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
            parts.push(format!("audience={}", urlencoding::encode(audience)));
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
    description: Vec<String>,
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
            total_expense, indoors, outdoors, created
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
    let tags: Vec<String> = tag_rows.into_iter().map(|r| r.tag).collect();

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

    // Get vote count
    let project_ulid = Ulid::from_string(project_id)?;
    let likes = crate::rating::ProjectRating::count_for_project(project_ulid).await?;

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

        // Check if user has liked this project
        let liked = crate::rating::ProjectRating::has_rated(project_ulid, user.user_id).await?;

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

    // Load reviews (limited to 5 most recent)
    let project_ulid = Ulid::from_string(project_id)?;
    let project_reviews = ProjectReview::list_for_project(project_ulid, Some(5)).await?;

    // Fetch user display info for all reviewers in batch
    let user_ids: Vec<Ulid> = project_reviews.iter().map(|r| r.user_id).collect();
    let user_info_map = if !user_ids.is_empty() {
        use common::message::user::{UserDisplayInfoBatchRequest, UserDisplayInfoBatchResponse};
        use std::collections::HashMap;

        let response: UserDisplayInfoBatchResponse = request(
            "user.info.display_info_batch",
            UserDisplayInfoBatchRequest { user_ids },
        )
        .await?;

        response
            .users
            .into_iter()
            .map(|u| (u.user_id, (u.handle, u.image)))
            .collect::<HashMap<_, _>>()
    } else {
        std::collections::HashMap::new()
    };

    // Enrich reviews with user display information
    let reviews: Vec<ReviewForDisplay> = project_reviews
        .into_iter()
        .map(|review| {
            let (user_handle, user_image) = user_info_map
                .get(&review.user_id)
                .cloned()
                .unwrap_or_else(|| (format!("({})", review.user_id), String::new()));

            ReviewForDisplay {
                user_id: review.user_id.to_string(),
                user_handle,
                user_image,
                text: review.text,
                when: review.reviewed_at,
            }
        })
        .collect();

    // Convert empty strings to None for optional fields
    let url = if row.url.is_empty() { None } else { Some(row.url) };
    let twitter_name = if row.twitter_name.is_empty() { None } else { Some(row.twitter_name) };
    let facebook_page = if row.facebook_page.is_empty() { None } else { Some(row.facebook_page) };

    Ok(ExchangeProjectDetail {
        id: project_slug.to_string(),
        slug: project_slug.to_string(),
        name: row.name,
        image: row.image,
        image_credit: row.image_credit,
        goal: row.goal,
        task: row.task,
        description: row.description,
        how_to_join: row.how_to_join,
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
    let page = 1;
    let page_size = 20;
    let offset = 0;

    // Build query for initial projects
    let mut query_builder = sqlx::QueryBuilder::new("");
    build_base_where_clause(&mut query_builder, exchange.info.exchange_id, &params, false);
    apply_search_filters(&mut query_builder, exchange.info.exchange_id, &params);

    // Add ordering and pagination
    query_builder.push(" ORDER BY p.created DESC LIMIT ");
    query_builder.push_bind(page_size as i64);
    query_builder.push(" OFFSET ");
    query_builder.push_bind(offset as i64);

    let rows = query_builder
        .build_query_as::<(String, String, String, String, String, String, String)>()
        .fetch_all(common::shared::db())
        .await?;

    let projects: Vec<ProjectSummary> = rows
        .into_iter()
        .map(|(_, slug, name, image, image_credit, goal, task)| ProjectSummary {
            slug,
            name,
            image,
            image_credit,
            goal,
            task,
        })
        .collect();

    // Get total count
    let mut count_builder = sqlx::QueryBuilder::new("");
    build_base_where_clause(&mut count_builder, exchange.info.exchange_id, &params, true);
    apply_search_filters(&mut count_builder, exchange.info.exchange_id, &params);

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
    Query(params): Query<SearchParams>,
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
    let has_text_search = !params.phrase.is_empty() && params.phrase.iter().any(|p| !p.is_empty());
    let mut query_builder = sqlx::QueryBuilder::new("");

    // Build base WHERE clause
    build_base_where_clause(&mut query_builder, exchange.info.exchange_id, &params, false);

    // Apply common search filters
    apply_search_filters(&mut query_builder, exchange.info.exchange_id, &params);

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
        .build_query_as::<(String, String, String, String, String, String, String)>()
        .fetch_all(common::shared::db())
        .await?;

    let projects: Vec<ProjectSummary> = rows
        .into_iter()
        .map(|(_, slug, name, image, image_credit, goal, task)| ProjectSummary {
            slug,
            name,
            image,
            image_credit,
            goal,
            task,
        })
        .collect();

    // Build count query with same filters
    let mut count_builder = sqlx::QueryBuilder::new("");

    // Build base WHERE clause for count
    build_base_where_clause(&mut count_builder, exchange.info.exchange_id, &params, true);

    // Add same filters as main query
    apply_search_filters(&mut count_builder, exchange.info.exchange_id, &params);

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
    Query(params): Query<SearchParams>,
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

    // Build query for ALL projects (no pagination) with same filters as search
    let has_text_search = !params.phrase.is_empty() && params.phrase.iter().any(|p| !p.is_empty());
    let mut query_builder = sqlx::QueryBuilder::new("");

    // Build base WHERE clause (no pagination - get all results)
    build_base_where_clause(&mut query_builder, exchange.exchange_id, &params, false);
    apply_search_filters(&mut query_builder, exchange.exchange_id, &params);

    // Add ordering (same as search results)
    if has_text_search {
        query_builder.push(" ORDER BY bm25(projects_fts), p.created DESC");
    } else {
        query_builder.push(" ORDER BY p.created DESC");
    }

    // Execute query (no LIMIT - get all results for CSV)
    let rows = query_builder
        .build_query_as::<(String, String, String, String, String, String, String)>()
        .fetch_all(common::shared::db())
        .await?;

    // Build CSV content
    let mut csv_content = String::new();
    csv_content.push_str("project_id,slug,name,image,image_credit,goal,task\n");

    for (project_id, slug, name, image, image_credit, goal, task) in rows {
        // Escape CSV fields (basic escaping - wrap in quotes if contains comma or quote)
        let escape_csv = |s: &str| -> String {
            if s.contains(',') || s.contains('"') || s.contains('\n') {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_string()
            }
        };

        csv_content.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            escape_csv(&project_id),
            escape_csv(&slug),
            escape_csv(&name),
            escape_csv(&image),
            escape_csv(&image_credit),
            escape_csv(&goal),
            escape_csv(&task)
        ));
    }

    Ok(poem::Response::builder()
        .header("Content-Type", "text/csv")
        .header("Content-Disposition", "attachment; filename=\"projects.csv\"")
        .body(csv_content))
}

#[derive(TemplateOnce)]
#[template(path = "exchange/admin.stpl.html")]
struct ExchangeAdminPage {
    exchange: ExchangeInfo,
    predefined_searches: Vec<super::PredefinedSearch>,
}

/// Exchange admin page
#[handler]
pub async fn exchange_admin(
    Path(exchange_id): Path<i64>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let exchange = ExchangeInfo::load_by_id(exchange_id).await?;

    // Check permission via user-service
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

    // Load predefined searches for admin UI
    let predefined_searches = exchange.get_predefined_searches().await?;

    let page = ExchangeAdminPage {
        exchange,
        predefined_searches,
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
}

/// Update exchange settings (uses LoadStorePeered for write)
#[handler]
pub async fn exchange_update_settings(
    Path(exchange_id): Path<i64>,
    form: poem::web::Form<ExchangeSettingsForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    // Load full Exchange via LoadStorePeered for modification
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

    // Update settings from form data
    exchange.settings.allow_project_voting = form.allow_project_voting;
    exchange.settings.allow_project_reviews = form.allow_project_reviews;
    exchange.settings.require_approval = form.require_approval;
    exchange.settings.restricted_add = form.restricted_add;
    exchange.settings.custom_css = if form.custom_css.is_empty() {
        None
    } else {
        Some(form.custom_css.clone())
    };

    exchange.updated = TimeString::now();

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

/// Create predefined search (uses LoadStorePeered for write)
#[handler]
pub async fn create_predefined_search(
    Path(exchange_id): Path<i64>,
    form: poem::web::Form<PredefinedSearchForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
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

    exchange.predefined_searches.push(new_search);
    exchange.updated = TimeString::now();
    exchange.store().await?;

    Ok(Redirect::see_other(format!("/exchange/{}/org.html", exchange.exchange_id)).into_response())
}

/// Delete predefined search (uses LoadStorePeered for write)
#[handler]
pub async fn delete_predefined_search(
    Path((exchange_id, search_id_str)): Path<(i64, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let search_id = Ulid::from_string(&search_id_str)?;
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

    // Remove the predefined search
    exchange
        .predefined_searches
        .retain(|s| s.search_id != search_id);

    exchange.updated = TimeString::now();
    exchange.store().await?;

    Ok(Html("Deleted").into_response())
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

    // Load project by slug
    let project_id_row = sqlx::query!(
        r#"
        SELECT p.project_id
        FROM projects p
        LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
        WHERE ent.slug = ? AND ent.publish = 1
        "#,
        project_slug
    )
    .fetch_optional(common::shared::db())
    .await?;

    let project_id = project_id_row
        .ok_or_else(|| anyhow::anyhow!("Project not found"))?
        .project_id;

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
    let project = load_exchange_project_detail(
        &project_id,
        &project_slug,
        auth.0.as_ref(),
        &exchange_info,
    )
    .await?;

    // Split description into paragraphs
    let description: Vec<String> = project
        .description
        .split("\n\n")
        .map(|s| s.to_string())
        .collect();

    // Get CSRF token
    let csrftoken = session
        .get::<String>("csrf-token")
        .unwrap_or_else(|| "".to_string());

    let user = TemplateUser::from(auth);

    let template = ExchangeProjectPage {
        exchange,
        user,
        project,
        description,
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
    use crate::rating::ProjectRating;
    use crate::peering::LoadStorePeered;

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

    let project_id = Ulid::from_string(&project_id_str)?;

    // Create and store the rating using LoadStorePeered
    let rating = ProjectRating::new(project_id, user_info.user_id);
    rating.store().await?;

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
    Path((exchange_id, project_slug)): Path<(i64, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
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
        exchange.updated = TimeString::now();
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
    exchange_id: i64,
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
