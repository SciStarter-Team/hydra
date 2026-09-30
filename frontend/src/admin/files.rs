// Files & Links management for admin

use crate::component::{AdminLayout, Component};
use crate::gourl::{GoUrl, GoUrlError, GoUrlType};
use crate::local_time::{TimeFormat, local_time};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::upload::storage::FileMetadata;
use common::message::log::{GoUrlStatsRequest, GoUrlStatsResponse};
use common::shared::db;
use poem::{
    Endpoint, IntoResponse, Route, delete, get, handler,
    web::{Form, Path, Query},
};
use sailfish::TemplateOnce;
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Row};

/// The message an HTMX form shows when a go link operation is refused.
fn go_url_error_response(error: GoUrlError) -> poem::Response {
    if let GoUrlError::Storage(e) = &error {
        tracing::error!("Go URL storage failed: {e:#}");
    }
    poem::Response::builder()
        .status(error.status())
        .content_type("text/html")
        .body(format!(
            r#"<div class="text-red-600">{}</div>"#,
            crate::component::html_escape(&error.to_string())
        ))
}

// Types
#[derive(Debug, Clone)]
pub struct GoUrlWithFileInfo {
    pub slug: String,
    pub url_type: GoUrlType,
    pub file_info: Option<FileMetadata>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/files_links.stpl.html")]
pub struct FilesLinksPage {
    pub go_urls_list: GoUrlsList,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/go_urls_list.stpl.html")]
pub struct GoUrlsList {
    pub go_urls: Vec<GoUrlWithFileInfo>,
    pub pagination: PaginationInfo,
    pub search_query: Option<String>,
}

#[derive(Debug)]
pub struct PaginationInfo {
    pub current_page: u32,
    pub per_page: u32,
    pub total_count: u32,
    pub total_pages: u32,
    pub has_prev: bool,
    pub has_next: bool,
}

#[derive(TemplateOnce)]
#[template(path = "admin/partials/upload_success_response.stpl.html")]
pub struct UploadSuccessResponse {
    pub slug: String,
    pub file_id: ulid::Ulid,
    pub go_urls_list: GoUrlsList,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/go_url_row.stpl.html")]
pub struct GoUrlRow<'a> {
    pub go_url: &'a GoUrlWithFileInfo,
    pub oob: bool,
    pub oob_swap: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/go_url_edit_row.stpl.html")]
pub struct GoUrlEditRow {
    pub slug: String,
    pub target_url: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/redirect_success_response.stpl.html")]
pub struct RedirectSuccessResponse {
    pub slug: String,
    pub go_urls_list: GoUrlsList,
}

#[derive(TemplateOnce)]
#[template(path = "admin/partials/go_url_stats_modal.stpl.html")]
pub struct GoUrlStatsModal {
    pub slug: String,
    pub total_views: i64,
    pub views_in_range: i64,
    pub chart_data: Vec<ChartPoint>,
    pub chart_max_views: i64,
    pub recent_views: Vec<RecentView>,
    pub date_range_start: String,
    pub date_range_end: String,
}

#[derive(Debug, Clone)]
pub struct ChartPoint {
    pub date: String,       // YYYY-MM-DD
    pub date_label: String, // e.g., "Sep 23"
    pub views: i64,
    pub height_percent: f64, // 0-100 for chart display
}

#[derive(Debug, Clone)]
pub struct RecentView {
    pub timestamp: String,
    pub ip_address: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/partials/error_modal.stpl.html")]
pub struct ErrorModal {
    pub message: String,
}

// Query and form types
#[derive(Serialize, Deserialize)]
struct SearchQuery {
    page: Option<u32>,
    per_page: Option<u32>,
    search: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct CreateRedirectRequest {
    slug: String,
    target_url: String,
}

/// The slug is not editable: it is the record's database key, so renaming is a
/// delete and a create rather than an update.
#[derive(Debug, Clone, Deserialize)]
struct UpdateRedirectRequest {
    target_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatsDateRange {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

// Helper functions
pub async fn get_go_urls_with_file_info_searched(
    page: u32,
    per_page: u32,
    search: Option<String>,
) -> Result<GoUrlsList, anyhow::Error> {
    // Build count query
    let mut count_query: QueryBuilder<sqlx::Sqlite> =
        QueryBuilder::new("SELECT COUNT(*) FROM go_urls");
    if let Some(ref search_term) = search {
        count_query.push(" WHERE slug LIKE ");
        count_query.push_bind(format!("%{}%", search_term));
    }

    let total_count: i64 = count_query.build_query_scalar().fetch_one(db()).await?;
    let total_count = total_count as u32;
    let total_pages = (total_count + per_page - 1) / per_page; // Ceiling division
    let offset = (page - 1) * per_page;

    // Build main query
    let mut query: QueryBuilder<sqlx::Sqlite> =
        QueryBuilder::new("SELECT slug, url_type FROM go_urls");
    if let Some(ref search_term) = search {
        query.push(" WHERE slug LIKE ");
        query.push_bind(format!("%{}%", search_term));
    }
    query.push(" ORDER BY slug LIMIT ");
    query.push_bind(per_page);
    query.push(" OFFSET ");
    query.push_bind(offset);

    let rows = query.build().fetch_all(db()).await?;

    let mut go_urls = Vec::new();

    for row in rows {
        let slug: String = row.get("slug");
        let url_type_blob: Vec<u8> = row.get("url_type");
        let url_type: GoUrlType = flexbuffers::from_slice(&url_type_blob)?;

        let file_info = match &url_type {
            GoUrlType::TrackedFile { file_id } => {
                // Get file metadata if this is a tracked file
                FileMetadata::load(*file_id).await.ok()
            }
            GoUrlType::Redirect { .. } => None,
        };

        go_urls.push(GoUrlWithFileInfo {
            slug,
            url_type,
            file_info,
        });
    }

    let pagination = PaginationInfo {
        current_page: page,
        per_page,
        total_count,
        total_pages,
        has_prev: page > 1,
        has_next: page < total_pages,
    };

    Ok(GoUrlsList {
        go_urls,
        pagination,
        search_query: search,
    })
}

pub async fn get_go_urls_with_file_info_paginated(
    page: u32,
    per_page: u32,
) -> Result<GoUrlsList, anyhow::Error> {
    get_go_urls_with_file_info_searched(page, per_page, None).await
}

pub async fn get_go_urls_with_file_info() -> Result<GoUrlsList, anyhow::Error> {
    get_go_urls_with_file_info_searched(1, 1000, None).await // Large page size for "all"
}

async fn fetch_go_url_stats(
    slug: String,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<GoUrlStatsModal, String> {
    use common::service::request;

    // Request stats from log service
    let stats_request = GoUrlStatsRequest {
        slug: slug.clone(),
        start_date,
        end_date,
    };

    match request::<GoUrlStatsResponse>("log.stats.go_url", stats_request).await {
        Ok(stats) => {
            let chart_data = stats
                .chart_data
                .into_iter()
                .map(|point| ChartPoint {
                    date: point.date,
                    date_label: point.date_label,
                    views: point.views,
                    height_percent: point.height_percent,
                })
                .collect();

            let recent_views = stats
                .recent_views
                .into_iter()
                .map(|view| RecentView {
                    timestamp: view.timestamp,
                    ip_address: view.ip_address,
                })
                .collect();

            Ok(GoUrlStatsModal {
                slug: stats.slug,
                total_views: stats.total_views,
                views_in_range: stats.views_in_range,
                chart_data,
                chart_max_views: stats.chart_max_views,
                recent_views,
                date_range_start: stats.date_range_start,
                date_range_end: stats.date_range_end,
            })
        }
        Err(e) => {
            tracing::error!("Failed to get go URL stats for {}: {}", slug, e);
            Err(format!("Failed to load stats for {}", slug))
        }
    }
}

// Handlers
#[handler]
async fn files_links(
    Authenticated(user_info): Authenticated,
    Query(query): Query<SearchQuery>,
) -> Result<poem::Response, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(20).clamp(1, 100); // Limit to reasonable range
    let search = query.search.filter(|s| !s.trim().is_empty());

    let go_urls_list = get_go_urls_with_file_info_searched(page, per_page, search.clone()).await?;

    AdminLayout::new("Files & Links", FilesLinksPage { go_urls_list })
        .with_breadcrumb("Files & Links", None)
        .with_sidebar_active("files-links")
        .into_response(user_info)
        .await
}

#[handler]
async fn files_links_search(
    Authenticated(_user_info): Authenticated,
    Query(query): Query<SearchQuery>,
) -> Result<impl IntoResponse, poem::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(20).clamp(1, 100);
    let search = query.search.filter(|s| !s.trim().is_empty());

    match get_go_urls_with_file_info_searched(page, per_page, search.clone()).await {
        Ok(go_urls_list) => {
            match go_urls_list.render_once() {
                Ok(rendered) => {
                    // Build the URL for HX-Push-Url
                    let mut url = String::from("/-admin/files-links");
                    let mut params = Vec::new();

                    if page > 1 {
                        params.push(format!("page={}", page));
                    }
                    if per_page != 20 {
                        params.push(format!("per_page={}", per_page));
                    }
                    if let Some(ref search_term) = search {
                        params.push(format!("search={}", urlencoding::encode(search_term)));
                    }

                    if !params.is_empty() {
                        url.push('?');
                        url.push_str(&params.join("&"));
                    }

                    Ok(poem::Response::builder()
                        .status(poem::http::StatusCode::OK)
                        .content_type("text/html")
                        .header("HX-Push-Url", url)
                        .body(rendered))
                }
                Err(e) => {
                    tracing::error!("Failed to render go URLs list: {}", e);
                    Err(poem::Error::from_string(
                        "Failed to render search results",
                        poem::http::StatusCode::INTERNAL_SERVER_ERROR,
                    ))
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to search go URLs: {}", e);
            Err(poem::Error::from_string(
                "Search failed",
                poem::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

#[handler]
async fn files_links_create_redirect(
    Form(request): Form<CreateRedirectRequest>,
    Authenticated(user_info): Authenticated,
) -> Result<impl IntoResponse, poem::Error> {
    // Save slug before it gets moved
    let slug = request.slug.clone();

    let go_url = GoUrl::new_redirect(request.slug, request.target_url);
    if let Err(error) = go_url.create().await {
        return Ok(go_url_error_response(error));
    }

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ = common::service::log_activity(
            None,
            "gourl-create",
            user_info.user_id,
            None,
            Some(slug.clone()),
        )
        .await;
    }

    // Return success message with form clearing and go URLs list update
    let go_urls_list = get_go_urls_with_file_info().await.map_err(|e| {
        tracing::error!("Failed to get go URLs: {}", e);
        poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let response = RedirectSuccessResponse { slug, go_urls_list };

    let rendered = response.render_once().map_err(|e| {
        tracing::error!("Failed to render template: {}", e);
        poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(rendered))
}

/// Render one go URL's row, either as the edit form or back as a plain row
///
/// Both the edit and cancel paths need the record fetched and rendered the same
/// way, so they share this.
async fn go_url_row_response(slug: &str, editing: bool) -> Result<poem::Response, poem::Error> {
    let go_url = match GoUrl::find(slug).await {
        Ok(go_url) => go_url,
        Err(error) => return Ok(go_url_error_response(error)),
    };

    // A tracked file's destination is the file itself, changed through the
    // upload routes; only a redirect has a target to edit here.
    let target_url = match &go_url.url_type {
        GoUrlType::Redirect { target_url } => target_url.clone(),
        GoUrlType::TrackedFile { .. } => {
            return Ok(go_url_error_response(GoUrlError::NotARedirect));
        }
    };

    let rendered = if editing {
        GoUrlEditRow {
            slug: go_url.slug.clone(),
            target_url,
        }
        .render_once()
    } else {
        let with_file_info = GoUrlWithFileInfo {
            slug: go_url.slug.clone(),
            url_type: go_url.url_type.clone(),
            file_info: None,
        };
        GoUrlRow {
            go_url: &with_file_info,
            oob: false,
            oob_swap: String::new(),
        }
        .render_once()
    };

    let rendered = rendered.map_err(|e| {
        tracing::error!("Failed to render go URL row: {}", e);
        poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(rendered))
}

#[handler]
async fn files_links_edit_go_url_form(
    Path(slug): Path<String>,
) -> Result<poem::Response, poem::Error> {
    go_url_row_response(&slug, true).await
}

#[handler]
async fn files_links_cancel_edit_go_url(
    Path(slug): Path<String>,
) -> Result<poem::Response, poem::Error> {
    go_url_row_response(&slug, false).await
}

#[handler]
async fn files_links_update_go_url(
    Path(slug): Path<String>,
    Form(request): Form<UpdateRedirectRequest>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, poem::Error> {
    // Load rather than rebuild: the record carries state-machine and session
    // fields imported from orrery that this form does not show, and
    // constructing a fresh one would silently discard them. The slug is the
    // database key, so it stays put --- renaming is a delete and a create.
    let mut go_url = match GoUrl::find(&slug).await {
        Ok(go_url) => go_url,
        Err(error) => return Ok(go_url_error_response(error)),
    };

    if !matches!(go_url.url_type, GoUrlType::Redirect { .. }) {
        return Ok(go_url_error_response(GoUrlError::NotARedirect));
    }

    go_url.url_type = GoUrlType::Redirect {
        target_url: request.target_url.trim().to_string(),
    };

    if let Err(error) = go_url.save().await {
        return Ok(go_url_error_response(error));
    }

    if let Some(user_info) = &user_info {
        let _ = common::service::log_activity(
            None,
            "gourl-edit",
            user_info.user_id,
            None,
            Some(slug.clone()),
        )
        .await;
    }

    go_url_row_response(&slug, false).await
}

#[handler]
async fn files_links_delete_go_url(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<impl IntoResponse, poem::Error> {
    if let Err(error) = GoUrl::remove(&slug).await {
        return Ok(go_url_error_response(error));
    }

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ = common::service::log_activity(
            None,
            "gourl-delete",
            user_info.user_id,
            None,
            Some(slug.clone()),
        )
        .await;
    }

    // Return empty response - HTMX will remove the targeted element
    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(""))
}

#[handler]
async fn files_links_get_stats(
    Path(slug): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, poem::Error> {
    match fetch_go_url_stats(slug, None, None).await {
        Ok(modal) => Ok(modal.render_once().unwrap()),
        Err(error_msg) => {
            let error_modal = ErrorModal {
                message: error_msg.clone(),
            };
            match error_modal.render_once() {
                Ok(rendered) => Ok(rendered),
                Err(e) => {
                    tracing::error!("Failed to render error modal: {}", e);
                    Ok(format!(
                        r#"<div class="p-4 bg-red-100 border border-red-300 rounded">Error: {}</div>"#,
                        error_msg
                    ))
                }
            }
        }
    }
}

#[handler]
async fn files_links_post_stats(
    Path(slug): Path<String>,
    Form(date_range): Form<StatsDateRange>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, poem::Error> {
    match fetch_go_url_stats(slug, date_range.start_date, date_range.end_date).await {
        Ok(modal) => Ok(modal.render_once().unwrap()),
        Err(error_msg) => {
            let error_modal = ErrorModal {
                message: error_msg.clone(),
            };
            match error_modal.render_once() {
                Ok(rendered) => Ok(rendered),
                Err(e) => {
                    tracing::error!("Failed to render error modal: {}", e);
                    Ok(format!(
                        r#"<div class="p-4 bg-red-100 border border-red-300 rounded">Error: {}</div>"#,
                        error_msg
                    ))
                }
            }
        }
    }
}

// Routes export
pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(files_links).post(files_links_create_redirect))
        .at("/search", get(files_links_search))
        .at(
            "/:slug",
            delete(files_links_delete_go_url).post(files_links_update_go_url),
        )
        .at("/:slug/edit", get(files_links_edit_go_url_form))
        .at("/:slug/row", get(files_links_cancel_edit_go_url))
        .at(
            "/:slug/stats",
            get(files_links_get_stats).post(files_links_post_stats),
        )
}
