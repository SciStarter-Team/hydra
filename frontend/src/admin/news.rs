use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use crate::news::{NewsItem, NewsItemSource};
use crate::peering::LoadStorePeered;
use common::shared::db;
use poem::{
    Endpoint, Route, get, handler,
    web::{Form, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

// Templates
#[derive(TemplateOnce)]
#[template(path = "admin/news.stpl.html")]
struct NewsPage {
    news_content: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/news_content.stpl.html")]
struct NewsContentTemplate {
    items: Vec<NewsItem>,
    search_query: Option<String>,
    page: usize,
    per_page: usize,
    total_count: usize,
}

struct NewsItemRow {
    news_id: String,
    source_type: NewsItemSource,
    tag: String,
    title: String,
    author: String,
    display_date: String,
    url: String,
    active: i64,
    sort_order: i64,
}

#[derive(TemplateOnce)]
#[template(path = "admin/modals/new_news.stpl.html")]
struct NewNewsForm {
    error_message: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/edit_news_content.stpl.html")]
struct EditNewsContent {
    item: NewsItem,
    error_message: Option<String>,
}

// Query and form types
#[derive(Deserialize)]
struct NewsSearchQuery {
    search: Option<String>,
    page: Option<usize>,
}

#[derive(Deserialize)]
struct CreateNewsForm {
    source_type: String,
    tag: Option<String>,
    title: Option<String>,
    content: Option<String>,
    wp_url: Option<String>,
    author: String,
    display_date: String,
    sort_order: Option<i32>,
    active: Option<String>,
}

#[derive(Deserialize)]
struct UpdateNewsForm {
    source_type: String,
    tag: Option<String>,
    title: Option<String>,
    content: Option<String>,
    wp_url: Option<String>,
    author: String,
    display_date: String,
    sort_order: Option<i32>,
    active: Option<String>,
}

// Helper functions
async fn news_search_impl(query: &NewsSearchQuery) -> Result<String, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page: i64 = 25;
    let offset = (page - 1) as i64 * per_page;

    let items_raw = if let Some(ref search) = query.search {
        let pattern = format!("%{}%", search);
        sqlx::query_as!(
            NewsItemRow,
            r#"SELECT news_id,
                      source_type as "source_type: NewsItemSource",
                      tag,
                      title,
                      author,
                      display_date,
                      url,
                      active,
                      sort_order
               FROM news_items
               WHERE title LIKE ?1 OR tag LIKE ?1
               ORDER BY sort_order ASC, display_date DESC
               LIMIT ?2 OFFSET ?3"#,
            pattern,
            per_page,
            offset
        )
        .fetch_all(db())
        .await?
    } else {
        sqlx::query_as!(
            NewsItemRow,
            r#"SELECT news_id,
                      source_type as "source_type: NewsItemSource",
                      tag,
                      title,
                      author,
                      display_date,
                      url,
                      active,
                      sort_order
               FROM news_items
               ORDER BY sort_order ASC, display_date DESC
               LIMIT ?1 OFFSET ?2"#,
            per_page,
            offset
        )
        .fetch_all(db())
        .await?
    };

    let items = items_raw
        .into_iter()
        .map(|row| -> Result<_, anyhow::Error> {
            Ok(NewsItem {
                news_id: row.news_id.parse()?,
                source_type: row.source_type,
                tag: row.tag,
                title: row.title,
                content: String::new(), // Not needed for list view
                author: row.author,
                display_date: row.display_date,
                url: row.url,
                active: row.active != 0,
                sort_order: row.sort_order as i32,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let total_count: i64 = if let Some(ref search) = query.search {
        let pattern = format!("%{}%", search);
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM news_items WHERE title LIKE ? OR tag LIKE ?",
            pattern,
            pattern
        )
        .fetch_one(db())
        .await?
    } else {
        sqlx::query_scalar!("SELECT COUNT(*) FROM news_items")
            .fetch_one(db())
            .await?
    };

    let template = NewsContentTemplate {
        items,
        search_query: query.search.clone(),
        page,
        per_page: per_page as usize,
        total_count: total_count as usize,
    };

    Ok(template.render_once()?)
}

// Handlers
#[handler]
async fn news_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let search_results = news_search_impl(&NewsSearchQuery {
        search: None,
        page: Some(1),
    })
    .await?;

    let content = format!(r#"<div id="news-content">{}</div>"#, search_results);

    let page = NewsPage {
        news_content: content,
    };

    let prepared = AdminLayout::new("News", page)
        .with_sidebar_active("news")
        .with_breadcrumb("News", None)
        .with_quill()
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn news_search(
    Query(query): Query<NewsSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = news_search_impl(&query).await?;
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(content))
}

#[handler]
async fn news_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let form = NewNewsForm {
        error_message: None,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}

#[handler]
async fn news_create(
    Form(form): Form<CreateNewsForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    match (async || -> Result<Ulid, anyhow::Error> {
        let source_type: NewsItemSource = form.source_type.parse()?;

        let (tag, title, content, url) = match source_type {
            NewsItemSource::WordPress => {
                let wp_url = form.wp_url.unwrap_or_default();
                if wp_url.trim().is_empty() {
                    return Err(anyhow::anyhow!("WordPress URL is required"));
                }
                (
                    String::from("SciStarter Blog"),
                    form.title.unwrap_or_default(),
                    wp_url.clone(),
                    wp_url,
                )
            }
            NewsItemSource::Custom => {
                let title = form.title.unwrap_or_default();
                if title.trim().is_empty() {
                    return Err(anyhow::anyhow!("Title is required"));
                }
                (
                    form.tag.unwrap_or_default(),
                    title,
                    form.content.unwrap_or_default(),
                    String::new(),
                )
            }
        };

        let news_item = NewsItem {
            news_id: Ulid::new(),
            source_type,
            tag,
            title,
            content,
            author: form.author,
            display_date: form.display_date,
            url,
            active: form.active.is_some(),
            sort_order: form.sort_order.unwrap_or(0),
        };

        let id = news_item.news_id;
        news_item.store().await?;

        if let Some(user_info) = &user_info {
            common::service::log_activity(
                None,
                "news-create",
                user_info.user_id,
                id,
                format!("source={}", source_type),
            )
            .await?;
        }

        Ok(id)
    })()
    .await
    {
        Ok(_id) => {
            let content = news_search_impl(&NewsSearchQuery {
                search: None,
                page: Some(1),
            })
            .await?;

            let html = format!(
                r#"<div id="news-content" hx-swap-oob="innerHTML">{}</div>"#,
                content
            );

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(html))
        }
        Err(e) => {
            let form = NewNewsForm {
                error_message: Some(e.to_string()),
            };

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(form.render_once()?))
        }
    }
}

#[handler]
async fn news_edit(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let news_id: Ulid = id_str.parse()?;
    let item = NewsItem::load(news_id).await?;

    let content = EditNewsContent {
        item,
        error_message: None,
    };

    let mut layout = AdminLayout::new("Edit News Item", content)
        .with_breadcrumb("News", Some("/-admin/news"))
        .with_breadcrumb("Edit", None)
        .with_sidebar_active("news");

    // Include Quill for custom content items
    layout = layout.with_quill();

    let prepared = layout.prepare(user_info).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn news_update(
    Path(id_str): Path<String>,
    Form(form): Form<UpdateNewsForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let news_id: Ulid = id_str.parse()?;

    match (async || -> Result<(), anyhow::Error> {
        let source_type: NewsItemSource = form.source_type.parse()?;

        let (tag, title, content, url) = match source_type {
            NewsItemSource::WordPress => {
                let wp_url = form.wp_url.unwrap_or_default();
                if wp_url.trim().is_empty() {
                    return Err(anyhow::anyhow!("WordPress URL is required"));
                }
                (
                    String::from("SciStarter Blog"),
                    form.title.unwrap_or_default(),
                    wp_url.clone(),
                    wp_url,
                )
            }
            NewsItemSource::Custom => {
                let title = form.title.unwrap_or_default();
                if title.trim().is_empty() {
                    return Err(anyhow::anyhow!("Title is required"));
                }
                (
                    form.tag.unwrap_or_default(),
                    title,
                    form.content.unwrap_or_default(),
                    String::new(),
                )
            }
        };

        let news_item = NewsItem {
            news_id,
            source_type,
            tag,
            title,
            content,
            author: form.author,
            display_date: form.display_date,
            url,
            active: form.active.is_some(),
            sort_order: form.sort_order.unwrap_or(0),
        };

        news_item.store().await?;

        if let Some(user_info) = &user_info {
            common::service::log_activity(
                None,
                "news-update",
                user_info.user_id,
                news_id,
                format!("source={}", source_type),
            )
            .await?;
        }

        Ok(())
    })()
    .await
    {
        Ok(()) => Ok(poem::Response::builder()
            .status(poem::http::StatusCode::SEE_OTHER)
            .header("Location", "/-admin/news")
            .finish()),
        Err(e) => {
            let item = NewsItem::load(news_id).await?;
            let content = EditNewsContent {
                item,
                error_message: Some(e.to_string()),
            };

            let layout = AdminLayout::new("Edit News Item", content)
                .with_breadcrumb("News", Some("/-admin/news"))
                .with_breadcrumb("Edit", None)
                .with_sidebar_active("news")
                .with_quill()
                .prepare(user_info)
                .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(layout.render_once()?))
        }
    }
}

#[handler]
async fn news_delete(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let news_id: Ulid = id_str.parse()?;

    let item = NewsItem::load(news_id).await.ok();

    NewsItem::delete(news_id).await?;

    if let Some(user_info) = &user_info {
        let extra = item.as_ref().map(|i| format!("title={}", i.title));
        common::service::log_activity(None, "news-delete", user_info.user_id, news_id, extra)
            .await?;
    }

    let exists: bool =
        sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM news_items) as "exists: bool""#)
            .fetch_one(db())
            .await?;

    let html = if !exists {
        let content = news_search_impl(&NewsSearchQuery {
            search: None,
            page: Some(1),
        })
        .await?;
        format!(
            r#"<div id="news-content" hx-swap-oob="outerHTML">{}</div>"#,
            content
        )
    } else {
        String::new()
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

// Routes export
pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(news_list).post(news_create))
        .at("/search", get(news_search))
        .at("/new", get(news_new))
        .at("/:id", get(news_edit).post(news_update).delete(news_delete))
}
