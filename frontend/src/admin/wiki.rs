// Wiki management for admin

use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::wiki::{WikiPage, render_wiki_content};
use poem::{
    Endpoint, IntoResponse, Route, get, handler,
    web::{Form, Path},
};
use sailfish::TemplateOnce;
use serde::Deserialize;

// Templates
#[derive(TemplateOnce)]
#[template(path = "admin/wiki_view.stpl.html")]
struct WikiPageView {
    title: String,
    content: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/wiki_edit.stpl.html")]
struct WikiPageEdit {
    title: String,
    content: String,
    is_new: bool,
}

// Form types
#[derive(Debug, Deserialize)]
struct WikiSaveForm {
    content: String,
}

// Handlers
#[handler]
async fn wiki_index(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Redirect to the Index page
    Ok(poem::web::Redirect::see_other("/-admin/wiki/Index").into_response())
}

/// GET /wiki/:title - Show wiki page or edit form if doesn't exist
#[handler]
async fn wiki_page(
    Path(title): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Try to load the page
    match WikiPage::load(title.clone()).await {
        Ok(page) => {
            // Page exists - show it with rendered wiki links
            let rendered_content = render_wiki_content(&page.content);

            let content = WikiPageView {
                title: page.title.clone(),
                content: rendered_content,
            };

            let prepared = AdminLayout::new(&format!("Wiki: {}", page.title), content)
                .with_sidebar_active("wiki")
                .prepare(user_info)
                .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(prepared.render_once()?))
        }
        Err(_) => {
            // Page doesn't exist - show editor
            let content = WikiPageEdit {
                title: title.clone(),
                content: String::new(),
                is_new: true,
            };

            let prepared = AdminLayout::new(&format!("Create Wiki Page: {}", title), content)
                .with_sidebar_active("wiki")
                .with_quill()
                .prepare(user_info)
                .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(prepared.render_once()?))
        }
    }
}

/// GET /wiki/:title/edit - Show edit form for existing page
#[handler]
async fn wiki_edit(
    Path(title): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Load the page (or use empty content if doesn't exist)
    let (content, is_new) = match WikiPage::load(title.clone()).await {
        Ok(page) => (page.content, false),
        Err(_) => (String::new(), true),
    };

    let edit_form = WikiPageEdit {
        title: title.clone(),
        content,
        is_new,
    };

    let page_title = if is_new {
        format!("Create Wiki Page: {}", title)
    } else {
        format!("Edit Wiki Page: {}", title)
    };

    let prepared = AdminLayout::new(&page_title, edit_form)
        .with_sidebar_active("wiki")
        .with_quill()
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

/// POST /wiki/:title - Save wiki page
#[handler]
async fn wiki_save(
    Path(title): Path<String>,
    Authenticated(user_info): Authenticated,
    form: Form<WikiSaveForm>,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let page = WikiPage {
        title: title.clone(),
        content: form.content.clone(),
    };

    page.store().await?;

    common::service::log_activity(
        None,
        "wiki-save",
        user_info.user_id,
        None,
        Some(title.clone()),
    )
    .await?;

    // Redirect back to view the page
    Ok(
        poem::web::Redirect::see_other(&format!("/-admin/wiki/{}", urlencoding::encode(&title)))
            .into_response(),
    )
}

/// DELETE /wiki/:title - Delete wiki page
#[handler]
async fn wiki_delete(
    Path(title): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let title_clone = title.clone();
    WikiPage::delete(title).await?;

    common::service::log_activity(
        None,
        "wiki-delete",
        user_info.user_id,
        None,
        Some(title_clone),
    )
    .await?;

    // Use HX-Redirect header for HTMX client-side redirect
    Ok(poem::Response::builder()
        .header("HX-Redirect", "/-admin/wiki")
        .status(poem::http::StatusCode::OK)
        .finish())
}

// Routes export
pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(wiki_index))
        .at(
            "/:title",
            get(wiki_page).post(wiki_save).delete(wiki_delete),
        )
        .at("/:title/edit", get(wiki_edit))
}
