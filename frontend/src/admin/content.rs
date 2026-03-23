// WYSIWYG and HTML content management for admin

use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use crate::pagecontent::{PageContent, PageContentKind};
use crate::peering::LoadStorePeered;
use common::shared::db;
use poem::{
    Endpoint, IntoResponse, Route, get, handler, post,
    web::{Form, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

// ==================== Shared Types ====================

#[derive(TemplateOnce)]
#[template(path = "admin/wysiwyg.stpl.html")]
struct PageContentWrapper {
    title: String,
    new_endpoint: String,
    content: String,
}

#[derive(Debug, Clone)]
struct WysiwygListItem {
    id: Ulid,
    name: String,
    excerpt: String,
    created_at: String,
}

#[derive(Debug, Clone)]
struct HtmlListItem {
    id: Ulid,
    name: String,
    excerpt: String,
    created_at: String,
}

#[derive(Deserialize)]
struct WysiwygSearchQuery {
    search: Option<String>,
    page: Option<usize>,
    per_page: Option<usize>,
}

#[derive(Deserialize)]
struct HtmlSearchQuery {
    search: Option<String>,
    page: Option<usize>,
    per_page: Option<usize>,
}

#[derive(Deserialize)]
struct CreatePageContentForm {
    kind: String,
    name: String,
    content: String,
}

#[derive(Deserialize)]
struct UpdatePageContentForm {
    name: String,
    content: String,
}

// ==================== Wysiwyg Templates ====================

#[derive(TemplateOnce)]
#[template(path = "admin/wysiwyg_content.stpl.html")]
struct WysiwygContentTemplate {
    items: Vec<WysiwygListItem>,
    search_query: Option<String>,
    page: usize,
    per_page: usize,
    total_count: usize,
}

#[derive(TemplateOnce)]
#[template(path = "admin/modals/new_wysiwyg.stpl.html")]
struct NewWysiwygForm {
    title: String,
    error_message: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/pages/edit_wysiwyg.stpl.html")]
struct EditWysiwygPage {
    id: Ulid,
    kind: PageContentKind,
    name: String,
    content: String,
    error_message: Option<String>,
}

// ==================== Dressed HTML Templates ====================

#[derive(TemplateOnce)]
#[template(path = "admin/dressed_html_content.stpl.html")]
struct DressedHtmlContentTemplate {
    items: Vec<HtmlListItem>,
    search_query: Option<String>,
    page: usize,
    per_page: usize,
    total_count: usize,
}

#[derive(TemplateOnce)]
#[template(path = "admin/modals/new_dressed_html.stpl.html")]
struct NewDressedHtmlForm {
    title: String,
    error_message: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/pages/edit_dressed_html.stpl.html")]
struct EditDressedHtmlPage {
    id: Ulid,
    kind: PageContentKind,
    name: String,
    content: String,
    error_message: Option<String>,
}

// ==================== Raw HTML Templates ====================

#[derive(TemplateOnce)]
#[template(path = "admin/raw_html_content.stpl.html")]
struct RawHtmlContentTemplate {
    items: Vec<HtmlListItem>,
    search_query: Option<String>,
    page: usize,
    per_page: usize,
    total_count: usize,
}

#[derive(TemplateOnce)]
#[template(path = "admin/modals/new_raw_html.stpl.html")]
struct NewRawHtmlForm {
    title: String,
    error_message: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/pages/edit_raw_html.stpl.html")]
struct EditRawHtmlPage {
    id: Ulid,
    kind: PageContentKind,
    name: String,
    content: String,
    error_message: Option<String>,
}

// ==================== Clear Modal Handler ====================

#[handler]
pub async fn clear_modal(
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, poem::Error> {
    // Return empty response to clear the modal container
    Ok("")
}

// ==================== Wysiwyg Handlers ====================

#[handler]
async fn wysiwyg_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let search_results = wysiwyg_search_impl(&WysiwygSearchQuery {
        search: None,
        page: Some(1),
        per_page: Some(25),
    })
    .await?;

    let content = format!(r#"<div id="wysiwyg-content">{}</div>"#, search_results);

    let page = PageContentWrapper {
        title: "Simple Pages".to_string(),
        new_endpoint: "/-admin/wysiwyg/new".to_string(),
        content,
    };

    let prepared = AdminLayout::new("Simple Content", page)
        .with_sidebar_active("wysiwyg")
        .with_breadcrumb("Simple Pages", None)
        .with_quill()
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

async fn wysiwyg_search_impl(query: &WysiwygSearchQuery) -> Result<String, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(25).clamp(10, 100);
    let offset = ((page - 1) * per_page) as i64;
    let per_page = per_page as i64;

    let search_pattern = query.search.as_ref().map(|s| format!("%{}%", s));
    let kind = "Wysiwyg";

    struct QueryRow {
        pagecontent_id: String,
        name: String,
        content: Vec<u8>,
    }

    let items: Vec<QueryRow> = if let Some(ref pattern) = search_pattern {
        sqlx::query_as!(
            QueryRow,
            r#"SELECT pagecontent_id, name, content FROM pagecontent
               WHERE kind = ? AND name LIKE ?
               ORDER BY pagecontent_id DESC
               LIMIT ? OFFSET ?"#,
            kind,
            pattern,
            per_page,
            offset
        )
        .fetch_all(db())
        .await?
    } else {
        sqlx::query_as!(
            QueryRow,
            r#"SELECT pagecontent_id, name, content FROM pagecontent
               WHERE kind = ?
               ORDER BY pagecontent_id DESC
               LIMIT ? OFFSET ?"#,
            kind,
            per_page,
            offset
        )
        .fetch_all(db())
        .await?
    };

    let total_count = if let Some(ref pattern) = search_pattern {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM pagecontent WHERE kind = 'Wysiwyg' AND name LIKE ?"#,
            pattern
        )
        .fetch_one(db())
        .await? as usize
    } else {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM pagecontent WHERE kind = 'Wysiwyg'"#
        )
        .fetch_one(db())
        .await? as usize
    };

    let items = items
        .into_iter()
        .map(|row| {
            let id: Ulid = row.pagecontent_id.parse().unwrap_or(Ulid::nil());
            let timestamp = jiff::Timestamp::from_millisecond(id.timestamp_ms() as i64).unwrap();
            let created_at = timestamp.strftime("%Y-%m-%d %H:%M").to_string();

            // Extract text excerpt from Delta JSON
            let excerpt = if let Ok(content_str) = std::str::from_utf8(&row.content) {
                common::quill::delta_to_excerpt(content_str, 100)
            } else {
                String::new()
            };

            WysiwygListItem {
                id,
                name: row.name,
                excerpt,
                created_at,
            }
        })
        .collect();

    let content = WysiwygContentTemplate {
        items,
        search_query: query.search.clone(),
        page: query.page.unwrap_or(1).max(1),
        per_page: query.per_page.unwrap_or(25).clamp(10, 100),
        total_count,
    };

    Ok(content.render_once()?)
}

#[handler]
async fn wysiwyg_search(
    Query(query): Query<WysiwygSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let html = wysiwyg_search_impl(&query).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn wysiwyg_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let form = NewWysiwygForm {
        title: "Create New Simple Page".to_string(),
        error_message: None,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}

#[handler]
async fn pagecontent_create(
    Form(form): Form<CreatePageContentForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind: PageContentKind = form.kind.parse()?;

    match (async || -> Result<Ulid, anyhow::Error> {
        if form.name.trim().is_empty() {
            return Err(anyhow::anyhow!("Name is required"));
        }

        let id = Ulid::new();
        let pagecontent =
            PageContent::new_with_id(id, kind, form.name.clone(), form.content.as_bytes());
        pagecontent.store().await?;

        Ok(id)
    })()
    .await
    {
        Ok(id) => {
            // Log the activity
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "pagecontent-create",
                    user_info.user_id,
                    id,
                    format!("kind={} name={}", form.kind, form.name),
                )
                .await?;
            }
            // Close modal and refresh list via OOB
            let (content, target_id) = match kind {
                PageContentKind::Wysiwyg => {
                    let content = wysiwyg_search_impl(&WysiwygSearchQuery {
                        search: None,
                        page: Some(1),
                        per_page: Some(25),
                    })
                    .await?;
                    (content, "wysiwyg-content")
                }
                PageContentKind::DressedHtml => {
                    let content = html_search_impl(
                        "DressedHtml",
                        &HtmlSearchQuery {
                            search: None,
                            page: Some(1),
                            per_page: Some(25),
                        },
                    )
                    .await?;
                    (content, "dressed-html-content")
                }
                PageContentKind::RawHtml => {
                    let content = html_search_impl(
                        "RawHtml",
                        &HtmlSearchQuery {
                            search: None,
                            page: Some(1),
                            per_page: Some(25),
                        },
                    )
                    .await?;
                    (content, "raw-html-content")
                }
                _ => return Err(anyhow::anyhow!("Unsupported page content kind")),
            };

            let html = format!(
                r#"<div id="{}" hx-swap-oob="innerHTML">{}</div>"#,
                target_id, content
            );

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(html))
        }
        Err(e) => {
            let html = match kind {
                PageContentKind::Wysiwyg => {
                    let form = NewWysiwygForm {
                        title: "Create New Simple Page".to_string(),
                        error_message: Some(e.to_string()),
                    };
                    form.render_once()?
                }
                PageContentKind::DressedHtml => {
                    let form = NewDressedHtmlForm {
                        title: "Create New Dressed HTML Page".to_string(),
                        error_message: Some(e.to_string()),
                    };
                    form.render_once()?
                }
                PageContentKind::RawHtml => {
                    let form = NewRawHtmlForm {
                        title: "Create New Raw HTML Page".to_string(),
                        error_message: Some(e.to_string()),
                    };
                    form.render_once()?
                }
                _ => return Err(anyhow::anyhow!("Unsupported page content kind")),
            };

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(html))
        }
    }
}

#[handler]
async fn pagecontent_edit(
    Path((kind_str, id_str)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind: PageContentKind = kind_str.parse()?;
    let id: Ulid = id_str.parse()?;

    let pagecontent = PageContent::load((kind, id)).await?;

    let content = EditWysiwygPage {
        id,
        kind,
        name: pagecontent.name().to_string(),
        content: pagecontent.as_utf8()?.to_string(),
        error_message: None,
    };

    let prepared = AdminLayout::new("Edit Simple Page", content)
        .with_sidebar_active("wysiwyg")
        .with_breadcrumb("Simple Pages", Some("/-admin/wysiwyg"))
        .with_breadcrumb("Edit", None)
        .with_quill()
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn pagecontent_update(
    Path((kind_str, id_str)): Path<(String, String)>,
    Form(form): Form<UpdatePageContentForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind: PageContentKind = kind_str.parse()?;
    let id: Ulid = id_str.parse()?;

    // Validate
    if form.name.trim().is_empty() {
        let content = EditWysiwygPage {
            id,
            kind,
            name: form.name,
            content: form.content,
            error_message: Some("Name is required".to_string()),
        };

        let prepared = AdminLayout::new("Edit Simple Page", content)
            .with_sidebar_active("wysiwyg")
            .with_breadcrumb("Simple Pages", Some("/-admin/wysiwyg"))
            .with_breadcrumb("Edit", None)
            .with_quill()
            .prepare(user_info)
            .await?;

        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(prepared.render_once()?));
    }

    // Update
    let mut pagecontent = PageContent::load((kind, id)).await?;
    pagecontent.set_name(form.name.clone());
    pagecontent.clear();
    pagecontent.extend_from_slice(form.content.as_bytes());
    pagecontent.store().await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            "pagecontent-update",
            user_info.user_id,
            id,
            format!("kind={:?} name={}", kind, form.name),
        )
        .await?;
    }

    // Redirect back to list page
    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", "/-admin/wysiwyg")
        .finish())
}

#[handler]
async fn pagecontent_delete(
    Path((kind_str, id_str)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind: PageContentKind = kind_str.parse()?;
    let id: Ulid = id_str.parse()?;
    let id_string = id.to_string();
    let kind_string = kind.as_ref();

    sqlx::query!(
        r#"DELETE FROM pagecontent WHERE pagecontent_id = ? AND kind = ?"#,
        id_string,
        kind_string,
    )
    .execute(db())
    .await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            "pagecontent-delete",
            user_info.user_id,
            id,
            format!("kind={:?}", kind),
        )
        .await?;
    }

    // Check if there are any items left
    let exists: bool = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM pagecontent WHERE kind = ?) as "exists: bool""#,
        kind_string
    )
    .fetch_one(db())
    .await?;

    let html = if !exists {
        // If no items left, return OOB swap with empty state
        let content = wysiwyg_search_impl(&WysiwygSearchQuery {
            search: None,
            page: Some(1),
            per_page: Some(25),
        })
        .await?;
        format!(
            r#"<div id="wysiwyg-content" hx-swap-oob="innerHTML">{}</div>"#,
            content
        )
    } else {
        // Otherwise just return empty to remove the row
        String::new()
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

// ==================== Dressed HTML Handlers ====================

#[handler]
async fn dressed_html_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let search_results = html_search_impl(
        "DressedHtml",
        &HtmlSearchQuery {
            search: None,
            page: Some(1),
            per_page: Some(25),
        },
    )
    .await?;

    let content = format!(r#"<div id="dressed-html-content">{}</div>"#, search_results);

    let page = PageContentWrapper {
        title: "Dressed HTML Pages".to_string(),
        new_endpoint: "/-admin/pagecontent/DressedHtml/new".to_string(),
        content,
    };

    let prepared = AdminLayout::new("Dressed HTML", page)
        .with_sidebar_active("engine-room-dressed-html")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("Dressed HTML", None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

async fn html_search_impl(kind: &str, query: &HtmlSearchQuery) -> Result<String, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(25).clamp(10, 100);
    let offset = ((page - 1) * per_page) as i64;
    let per_page_i64 = per_page as i64;

    let search_pattern = query.search.as_ref().map(|s| format!("%{}%", s));

    struct QueryRow {
        pagecontent_id: String,
        name: String,
        content: Vec<u8>,
    }

    let items: Vec<QueryRow> = if let Some(ref pattern) = search_pattern {
        sqlx::query_as!(
            QueryRow,
            r#"SELECT pagecontent_id, name, content FROM pagecontent
               WHERE kind = ? AND name LIKE ?
               ORDER BY pagecontent_id DESC
               LIMIT ? OFFSET ?"#,
            kind,
            pattern,
            per_page_i64,
            offset
        )
        .fetch_all(db())
        .await?
    } else {
        sqlx::query_as!(
            QueryRow,
            r#"SELECT pagecontent_id, name, content FROM pagecontent
               WHERE kind = ?
               ORDER BY pagecontent_id DESC
               LIMIT ? OFFSET ?"#,
            kind,
            per_page_i64,
            offset
        )
        .fetch_all(db())
        .await?
    };

    let total_count = if let Some(ref pattern) = search_pattern {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM pagecontent WHERE kind = ? AND name LIKE ?"#,
            kind,
            pattern
        )
        .fetch_one(db())
        .await? as usize
    } else {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM pagecontent WHERE kind = ?"#,
            kind
        )
        .fetch_one(db())
        .await? as usize
    };

    let items = items
        .into_iter()
        .map(|row| {
            let id: Ulid = row.pagecontent_id.parse().unwrap_or(Ulid::nil());
            let timestamp = jiff::Timestamp::from_millisecond(id.timestamp_ms() as i64).unwrap();
            let created_at = timestamp.strftime("%Y-%m-%d %H:%M").to_string();

            // Extract text excerpt from HTML
            let excerpt = if let Ok(content_str) = std::str::from_utf8(&row.content) {
                let text = content_str.chars().take(100).collect::<String>();
                text.trim().to_string()
            } else {
                String::new()
            };

            HtmlListItem {
                id,
                name: row.name,
                excerpt,
                created_at,
            }
        })
        .collect();

    let content = if kind == "DressedHtml" {
        DressedHtmlContentTemplate {
            items,
            search_query: query.search.clone(),
            page,
            per_page,
            total_count,
        }
        .render_once()?
    } else {
        RawHtmlContentTemplate {
            items,
            search_query: query.search.clone(),
            page,
            per_page,
            total_count,
        }
        .render_once()?
    };

    Ok(content)
}

#[handler]
async fn dressed_html_search(
    Query(query): Query<HtmlSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let html = html_search_impl("DressedHtml", &query).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn dressed_html_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let form = NewDressedHtmlForm {
        title: "Create New Dressed HTML Page".to_string(),
        error_message: None,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}

#[handler]
async fn dressed_html_edit(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = PageContentKind::DressedHtml;
    let id: Ulid = id_str.parse()?;

    let pagecontent = PageContent::load((kind, id)).await?;

    let content = EditDressedHtmlPage {
        id,
        kind,
        name: pagecontent.name().to_string(),
        content: pagecontent.as_utf8()?.to_string(),
        error_message: None,
    };

    let prepared = AdminLayout::new("Edit Dressed HTML", content)
        .with_sidebar_active("engine-room-dressed-html")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("Dressed HTML", Some("/-admin/pagecontent/DressedHtml"))
        .with_breadcrumb("Edit", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn dressed_html_update(
    Path(id_str): Path<String>,
    Form(form): Form<UpdatePageContentForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = PageContentKind::DressedHtml;
    let id: Ulid = id_str.parse()?;

    // Validate
    if form.name.trim().is_empty() {
        let content = EditDressedHtmlPage {
            id,
            kind,
            name: form.name,
            content: form.content,
            error_message: Some("Name is required".to_string()),
        };

        let prepared = AdminLayout::new("Edit Dressed HTML", content)
            .with_sidebar_active("engine-room-dressed-html")
            .with_breadcrumb("Engine Room", None)
            .with_breadcrumb("Dressed HTML", Some("/-admin/pagecontent/DressedHtml"))
            .with_breadcrumb("Edit", None)
            .prepare(user_info)
            .await?;

        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(prepared.render_once()?));
    }

    // Update
    let mut pagecontent = PageContent::load((kind, id)).await?;
    pagecontent.set_name(form.name);
    pagecontent.clear();
    pagecontent.extend_from_slice(form.content.as_bytes());
    pagecontent.store().await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ =
            common::service::log_activity(None, "dressedhtml-edit", user_info.user_id, id, None)
                .await;
    }

    // Redirect back to list page
    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", "/-admin/pagecontent/DressedHtml")
        .finish())
}

#[handler]
async fn dressed_html_delete(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = PageContentKind::DressedHtml;
    let id: Ulid = id_str.parse()?;
    let id_string = id.to_string();
    let kind_string = kind.as_ref();

    sqlx::query!(
        r#"DELETE FROM pagecontent WHERE pagecontent_id = ? AND kind = ?"#,
        id_string,
        kind_string
    )
    .execute(db())
    .await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ =
            common::service::log_activity(None, "dressedhtml-delete", user_info.user_id, id, None)
                .await;
    }

    // Check if there are any items left
    let exists: bool = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM pagecontent WHERE kind = ?) as "exists: bool""#,
        kind_string
    )
    .fetch_one(db())
    .await?;

    let html = if !exists {
        // If no items left, return OOB swap with empty state
        let content = html_search_impl(
            "DressedHtml",
            &HtmlSearchQuery {
                search: None,
                page: Some(1),
                per_page: Some(25),
            },
        )
        .await?;
        format!(
            r#"<div id="dressed-html-content" hx-swap-oob="innerHTML">{}</div>"#,
            content
        )
    } else {
        // Otherwise just return empty to remove the row
        String::new()
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

// ==================== Raw HTML Handlers ====================

#[handler]
async fn raw_html_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let search_results = html_search_impl(
        "RawHtml",
        &HtmlSearchQuery {
            search: None,
            page: Some(1),
            per_page: Some(25),
        },
    )
    .await?;

    let content = format!(r#"<div id="raw-html-content">{}</div>"#, search_results);

    let page = PageContentWrapper {
        title: "Raw HTML Pages".to_string(),
        new_endpoint: "/-admin/pagecontent/RawHtml/new".to_string(),
        content,
    };

    let prepared = AdminLayout::new("Raw HTML", page)
        .with_sidebar_active("engine-room-raw-html")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("Raw HTML", None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn raw_html_search(
    Query(query): Query<HtmlSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let html = html_search_impl("RawHtml", &query).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn raw_html_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let form = NewRawHtmlForm {
        title: "Create New Raw HTML Page".to_string(),
        error_message: None,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}

#[handler]
async fn raw_html_edit(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = PageContentKind::RawHtml;
    let id: Ulid = id_str.parse()?;

    let pagecontent = PageContent::load((kind, id)).await?;

    let content = EditRawHtmlPage {
        id,
        kind,
        name: pagecontent.name().to_string(),
        content: pagecontent.as_utf8()?.to_string(),
        error_message: None,
    };

    let prepared = AdminLayout::new("Edit Raw HTML", content)
        .with_sidebar_active("engine-room-raw-html")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("Raw HTML", Some("/-admin/pagecontent/RawHtml"))
        .with_breadcrumb("Edit", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn raw_html_update(
    Path(id_str): Path<String>,
    Form(form): Form<UpdatePageContentForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = PageContentKind::RawHtml;
    let id: Ulid = id_str.parse()?;

    // Validate
    if form.name.trim().is_empty() {
        let content = EditRawHtmlPage {
            id,
            kind,
            name: form.name,
            content: form.content,
            error_message: Some("Name is required".to_string()),
        };

        let prepared = AdminLayout::new("Edit Raw HTML", content)
            .with_sidebar_active("engine-room-raw-html")
            .with_breadcrumb("Engine Room", None)
            .with_breadcrumb("Raw HTML", Some("/-admin/pagecontent/RawHtml"))
            .with_breadcrumb("Edit", None)
            .prepare(user_info)
            .await?;

        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(prepared.render_once()?));
    }

    // Update
    let mut pagecontent = PageContent::load((kind, id)).await?;
    pagecontent.set_name(form.name);
    pagecontent.clear();
    pagecontent.extend_from_slice(form.content.as_bytes());
    pagecontent.store().await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ =
            common::service::log_activity(None, "rawhtml-edit", user_info.user_id, id, None).await;
    }

    // Redirect back to list page
    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", "/-admin/pagecontent/RawHtml")
        .finish())
}

#[handler]
async fn raw_html_delete(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = PageContentKind::RawHtml;
    let id: Ulid = id_str.parse()?;
    let id_string = id.to_string();
    let kind_string = kind.as_ref();

    sqlx::query!(
        r#"DELETE FROM pagecontent WHERE pagecontent_id = ? AND kind = ?"#,
        id_string,
        kind_string
    )
    .execute(db())
    .await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        let _ = common::service::log_activity(None, "rawhtml-delete", user_info.user_id, id, None)
            .await;
    }

    // Check if there are any items left
    let exists: bool = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM pagecontent WHERE kind = ?) as "exists: bool""#,
        kind_string
    )
    .fetch_one(db())
    .await?;

    let html = if !exists {
        // If no items left, return OOB swap with empty state
        let content = html_search_impl(
            "RawHtml",
            &HtmlSearchQuery {
                search: None,
                page: Some(1),
                per_page: Some(25),
            },
        )
        .await?;
        format!(
            r#"<div id="raw-html-content" hx-swap-oob="innerHTML">{}</div>"#,
            content
        )
    } else {
        // Otherwise just return empty to remove the row
        String::new()
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

// ==================== Routes ====================

pub fn wysiwyg_routes() -> impl Endpoint {
    Route::new()
        .at("/", get(wysiwyg_list))
        .at("/search", get(wysiwyg_search))
        .at("/new", get(wysiwyg_new))
}

pub fn pagecontent_routes() -> impl Endpoint {
    Route::new()
        .at("/", post(pagecontent_create))
        .at("/DressedHtml", get(dressed_html_list))
        .at("/DressedHtml/search", get(dressed_html_search))
        .at("/DressedHtml/new", get(dressed_html_new))
        .at(
            "/DressedHtml/:id",
            get(dressed_html_edit)
                .post(dressed_html_update)
                .delete(dressed_html_delete),
        )
        .at("/RawHtml", get(raw_html_list))
        .at("/RawHtml/search", get(raw_html_search))
        .at("/RawHtml/new", get(raw_html_new))
        .at(
            "/RawHtml/:id",
            get(raw_html_edit)
                .post(raw_html_update)
                .delete(raw_html_delete),
        )
        .at(
            "/:kind/:id",
            get(pagecontent_edit)
                .post(pagecontent_update)
                .delete(pagecontent_delete),
        )
}

pub fn clear_modal_handler() -> impl Endpoint {
    Route::new().at("/", post(clear_modal))
}
