// Archive management for admin

use crate::component::{AdminLayout, Component, admin_layout::AdminLayoutPrepared};
use crate::local_time::{TimeFormat, local_time};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use common::message::user::BasicInfo;
use common::shared::db;
use poem::{
    Endpoint, Route, get, handler, post,
    web::{Form, Path, Query},
};
use sailfish::TemplateOnce;
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Row};
use std::sync::Arc;
use ulid::Ulid;

// Archive Management Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveEntry {
    pub archive_id: Ulid,
    pub name: String,
    pub timestamp: String,
    pub notes: Option<String>,
    pub file_id: Option<Ulid>,
    pub snapshot_html: Option<String>,
    pub snapshot_url: Option<String>,
}

impl LoadStorePeered for ArchiveEntry {
    type DatabaseKey = Ulid;
    const GROUP: &str = "archive";
    const SPECIFIC: &str = "entry";

    async fn save_to_database(&self) -> Result<(), anyhow::Error> {
        let archive_id_str = self.archive_id.to_string();
        let file_id_str = self.file_id.map(|id| id.to_string());

        sqlx::query!(
            r#"INSERT OR REPLACE INTO archive_entries
               (archive_id, name, timestamp, notes, file_id, snapshot_html, snapshot_url)
               VALUES (?, ?, ?, ?, ?, ?, ?)"#,
            archive_id_str,
            self.name,
            self.timestamp,
            self.notes,
            file_id_str,
            self.snapshot_html,
            self.snapshot_url
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, anyhow::Error> {
        let archive_id_str = key.to_string();
        let row = sqlx::query!(
            r#"SELECT archive_id, name, timestamp, notes, file_id, snapshot_html, snapshot_url
               FROM archive_entries
               WHERE archive_id = ?"#,
            archive_id_str
        )
        .fetch_optional(db())
        .await?
        .ok_or_else(|| anyhow::anyhow!("Archive entry not found"))?;

        Ok(ArchiveEntry {
            archive_id: row.archive_id.parse()?,
            name: row.name,
            timestamp: row.timestamp,
            notes: row.notes,
            file_id: row.file_id.and_then(|id| id.parse().ok()),
            snapshot_html: row.snapshot_html,
            snapshot_url: row.snapshot_url,
        })
    }

    async fn delete_from_database(key: Self::DatabaseKey) -> Result<(), anyhow::Error> {
        let archive_id_str = key.to_string();
        sqlx::query!(
            r#"DELETE FROM archive_entries WHERE archive_id = ?"#,
            archive_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }
}

// Templates
#[derive(Debug)]
pub struct ArchivePage;

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/archive.stpl.html")]
pub struct ArchivePagePrepared {
    entries: Vec<ArchiveEntry>,
    current_page: u32,
    total_entries: u32,
    has_previous: bool,
    has_next: bool,
    per_page: u32,
    start_item: u32,
    end_item: u32,
    pagination_url: String,
    pagination_include: String,
    search_term: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/archive_content.stpl.html")]
pub struct ArchiveContent {
    pub entries: Vec<ArchiveEntry>,
    pub current_page: u32,
    pub total_entries: u32,
    pub has_previous: bool,
    pub has_next: bool,
    pub per_page: u32,
    pub start_item: u32,
    pub end_item: u32,
    pub pagination_url: String,
    pub pagination_include: String,
    pub search_term: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/archive_create.stpl.html")]
pub struct ArchiveCreatePage {}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/archive_edit.stpl.html")]
pub struct ArchiveEditPage {
    pub entry: ArchiveEntry,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/archive_view.stpl.html")]
pub struct ArchiveViewPage {
    pub entry: ArchiveEntry,
}

impl Component for ArchivePage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared<ArchivePagePrepared>;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let page = 1u32;
        let per_page = 25u32;

        let entries_list = match fetch_archive_entries(None, Some(page), Some(per_page)).await {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!("Failed to fetch archive entries from database: {}", e);
                vec![]
            }
        };

        let total_entries = count_archive_entries(None).await.unwrap_or(0);

        let archive_content = ArchivePagePrepared {
            entries: entries_list,
            current_page: page,
            total_entries: total_entries as u32,
            has_previous: false,
            has_next: total_entries > per_page as u64,
            per_page,
            start_item: 1,
            end_item: std::cmp::min(per_page, total_entries as u32),
            pagination_url: "/-admin/archive/search".to_string(),
            pagination_include: "[name='search']".to_string(),
            search_term: "".to_string(),
        };

        AdminLayout::new("Archive", archive_content)
            .with_breadcrumb("Archive", None)
            .with_sidebar_active("archive")
            .prepare(param)
            .await
    }
}

// Helper functions
async fn fetch_archive_entries(
    search: Option<&str>,
    page: Option<u32>,
    per_page: Option<u32>,
) -> Result<Vec<ArchiveEntry>, anyhow::Error> {
    let page = page.unwrap_or(1);
    let per_page = per_page.unwrap_or(25);
    let offset = (page - 1) * per_page;

    let mut query = QueryBuilder::new("");

    if let Some(search_term) = search.filter(|s| !s.trim().is_empty()) {
        query.push(
            "SELECT a.archive_id, a.name, a.timestamp, a.notes, a.file_id,
                    a.snapshot_html, a.snapshot_url
             FROM archive_entries a
             JOIN archive_entries_fts f ON a.archive_id = f.archive_id
             WHERE archive_entries_fts MATCH ",
        );
        query.push_bind(search_term);
    } else {
        query.push(
            "SELECT archive_id, name, timestamp, notes, file_id,
                    snapshot_html, snapshot_url
             FROM archive_entries
             WHERE 1=1",
        );
    }

    query.push(" ORDER BY timestamp DESC LIMIT ");
    query.push_bind(per_page);
    query.push(" OFFSET ");
    query.push_bind(offset);

    let results = query.build().fetch_all(db()).await?;

    let mut entries = Vec::new();
    for row in results {
        let archive_id: String = row.get("archive_id");
        let name: String = row.get("name");
        let timestamp: String = row.get("timestamp");
        let notes: Option<String> = row.get("notes");
        let file_id: Option<String> = row.get("file_id");
        let snapshot_html: Option<String> = row.get("snapshot_html");
        let snapshot_url: Option<String> = row.get("snapshot_url");

        entries.push(ArchiveEntry {
            archive_id: archive_id.parse()?,
            name,
            timestamp,
            notes,
            file_id: file_id.and_then(|id| id.parse().ok()),
            snapshot_html,
            snapshot_url,
        });
    }

    Ok(entries)
}

async fn count_archive_entries(search: Option<&str>) -> Result<u64, anyhow::Error> {
    let mut query = QueryBuilder::new("");

    if let Some(search_term) = search.filter(|s| !s.trim().is_empty()) {
        query.push(
            "SELECT COUNT(*) as count
             FROM archive_entries a
             JOIN archive_entries_fts f ON a.archive_id = f.archive_id
             WHERE archive_entries_fts MATCH ",
        );
        query.push_bind(search_term);
    } else {
        query.push("SELECT COUNT(*) as count FROM archive_entries WHERE 1=1");
    }

    let row = query.build().fetch_one(db()).await?;
    let count: i64 = row.get("count");

    Ok(count as u64)
}

async fn capture_page_snapshot(url: &str) -> Result<String, anyhow::Error> {
    // Use reqwest to fetch the page content
    let response = reqwest::get(url).await?;
    let html = response.text().await?;
    Ok(html)
}

// Query and form types
#[derive(Debug, Deserialize)]
struct ArchiveSearchQuery {
    search: Option<String>,
    page: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ArchiveCreateForm {
    name: String,
    timestamp: String,
    notes: Option<String>,
    file_id: Option<String>,
    snapshot_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArchiveEditForm {
    name: String,
    timestamp: String,
    notes: Option<String>,
    file_id: Option<String>,
    snapshot_url: Option<String>,
    capture_snapshot: Option<String>, // "true" if we should capture a new snapshot
}

// Handlers
#[handler]
async fn archive_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    Ok(ArchivePage.into_response(user_info).await?)
}

#[handler]
async fn archive_search(
    Query(query): Query<ArchiveSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let page = query.page.unwrap_or(1);
    let per_page = 25u32;
    let search_term = query.search.as_deref().unwrap_or("");

    let entries_list = fetch_archive_entries(Some(search_term), Some(page), Some(per_page)).await?;
    let total_entries = count_archive_entries(Some(search_term)).await? as u32;

    let archive_content = ArchiveContent {
        entries: entries_list,
        current_page: page,
        total_entries,
        has_previous: page > 1,
        has_next: (page * per_page) < total_entries,
        per_page,
        start_item: ((page - 1) * per_page) + 1,
        end_item: std::cmp::min(page * per_page, total_entries),
        pagination_url: "/-admin/archive/search".to_string(),
        pagination_include: "[name='search']".to_string(),
        search_term: search_term.to_string(),
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(archive_content.render_once()?))
}

#[handler]
async fn archive_create_form(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = ArchiveCreatePage {};

    let prepared = AdminLayout::new("Create Archive Entry", content)
        .with_breadcrumb("Archive", Some("/-admin/archive"))
        .with_breadcrumb("Create", None)
        .with_sidebar_active("archive")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn archive_create(
    Form(form): Form<ArchiveCreateForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    // Capture snapshot if URL provided
    let snapshot_html = if let Some(ref url) = form.snapshot_url {
        capture_page_snapshot(url).await.ok()
    } else {
        None
    };

    let entry = ArchiveEntry {
        archive_id: Ulid::new(),
        name: form.name,
        timestamp: form.timestamp,
        notes: form.notes.filter(|s| !s.trim().is_empty()),
        file_id: form.file_id.and_then(|id| id.parse().ok()),
        snapshot_html,
        snapshot_url: form.snapshot_url.filter(|s| !s.trim().is_empty()),
    };

    entry.store().await?;

    common::service::log_activity(
        None,
        "archive-create",
        user_info.user_id,
        Some(entry.archive_id),
        None,
    )
    .await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", "/-admin/archive")
        .finish())
}

#[handler]
async fn archive_view(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let archive_id: Ulid = id.parse()?;
    let entry = ArchiveEntry::load(archive_id).await?;

    let content = ArchiveViewPage { entry };

    let prepared = AdminLayout::new("View Archive Entry", content)
        .with_breadcrumb("Archive", Some("/-admin/archive"))
        .with_breadcrumb("View", None)
        .with_sidebar_active("archive")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn archive_snapshot(
    Path(id): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let archive_id: Ulid = id.parse()?;
    let entry = ArchiveEntry::load(archive_id).await?;

    if let Some(html) = entry.snapshot_html {
        Ok(poem::Response::builder()
            .content_type("text/html; charset=utf-8")
            .body(html))
    } else {
        Ok(poem::Response::builder()
            .status(poem::http::StatusCode::NOT_FOUND)
            .body("No snapshot available for this entry"))
    }
}

#[handler]
async fn archive_edit_form(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let archive_id: Ulid = id.parse()?;
    let entry = ArchiveEntry::load(archive_id).await?;

    let content = ArchiveEditPage { entry };

    let prepared = AdminLayout::new("Edit Archive Entry", content)
        .with_breadcrumb("Archive", Some("/-admin/archive"))
        .with_breadcrumb("Edit", None)
        .with_sidebar_active("archive")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn archive_edit(
    Path(id): Path<String>,
    Form(form): Form<ArchiveEditForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let archive_id: Ulid = id.parse()?;
    let mut entry = ArchiveEntry::load(archive_id).await?;

    // Update fields
    entry.name = form.name;
    entry.timestamp = form.timestamp;
    entry.notes = form.notes.filter(|s| !s.trim().is_empty());
    entry.file_id = form.file_id.and_then(|id| id.parse().ok());
    entry.snapshot_url = form.snapshot_url.filter(|s| !s.trim().is_empty());

    // Capture new snapshot if requested
    if form.capture_snapshot.as_deref() == Some("true") {
        if let Some(ref url) = entry.snapshot_url {
            entry.snapshot_html = capture_page_snapshot(url).await.ok();
        }
    }

    entry.store().await?;

    common::service::log_activity(
        None,
        "archive-edit",
        user_info.user_id,
        Some(entry.archive_id),
        None,
    )
    .await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", format!("/-admin/archive/{}", id))
        .finish())
}

#[handler]
async fn archive_delete(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let archive_id: Ulid = id.parse()?;
    ArchiveEntry::delete(archive_id).await?;

    common::service::log_activity(
        None,
        "archive-delete",
        user_info.user_id,
        Some(archive_id),
        None,
    )
    .await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", "/-admin/archive")
        .finish())
}

// Routes export
pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(archive_list))
        .at("/search", get(archive_search))
        .at("/new", get(archive_create_form).post(archive_create))
        .at("/:id", get(archive_view))
        .at("/:id/snapshot", get(archive_snapshot))
        .at("/:id/edit", get(archive_edit_form).post(archive_edit))
        .at("/:id/delete", post(archive_delete))
}
