// Page Builder administration - components, pages, slots, and engine room

use poem::{
    Route, get, handler, post,
    web::{Path, Query},
};
use sailfish::TemplateOnce;
use std::sync::Arc;

use crate::component::{AdminLayout, Component, admin_layout::AdminLayoutPrepared};
use crate::local_time::{TimeFormat, local_time};
use crate::middleware::authorization::Authenticated;
use common::message::user::BasicInfo;

// ============================================================================
// Engine Room
// ============================================================================

#[derive(Debug, Clone)]
pub struct EngineOperation {
    pub id: String,
    pub title: String,
    pub description: String,
    pub endpoint: String,
    pub button_text: String,
    pub icon: String,
    pub color: String,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/engine_room.stpl.html")]
pub struct EngineRoomPrepared {
    operations: Vec<EngineOperation>,
}

#[derive(Debug, Clone)]
pub struct EngineRoomPage;

impl Component for EngineRoomPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = AdminLayoutPrepared<EngineRoomPrepared>;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let operations = vec![
            EngineOperation {
                id: "sync-page-managers".to_string(),
                title: "Page Manager Sync".to_string(),
                description: "Synchronize page manager flags for all users based on current project and event editors.".to_string(),
                endpoint: "/-admin/engine-room/sync-page-managers".to_string(),
                button_text: "Run Bulk Sync".to_string(),
                icon: r#"<svg class="h-6 w-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z"></path>
                </svg>"#.to_string(),
                color: "blue".to_string(),
            },
        ];

        let engine_room_content = EngineRoomPrepared { operations };

        AdminLayout::new("Engine Room", engine_room_content)
            .with_breadcrumb("Engine Room", None)
            .with_sidebar_active("engine-room")
            .prepare(param)
            .await
    }
}

#[handler]
#[tracing::instrument]
async fn engine_room(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    EngineRoomPage.into_response(user_info).await
}

#[handler]
#[tracing::instrument]
async fn engine_room_sync_page_managers(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    match crate::page_manager::full_page_editor_sync().await {
        Ok(count) => {
            let success_html = format!(
                r#"<div class="p-3 text-sm text-green-700 bg-green-100 rounded-lg border border-green-200">
                    <strong>Success:</strong> Updated {} user page manager statuses
                </div>"#,
                count
            );
            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(success_html))
        }
        Err(e) => {
            let error_html = format!(
                r#"<div class="p-3 text-sm text-red-700 bg-red-100 rounded-lg border border-red-200">
                    <strong>Error:</strong> Failed to sync page managers: {}
                </div>"#,
                e
            );
            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(error_html))
        }
    }
}

// ============================================================================
// Page Builder Components CRUD
// ============================================================================

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pagebuilder/components_list.stpl.html")]
struct PbComponentsListPage {
    components: Vec<crate::pagebuilder::PageBuilderComponent>,
    categories: Vec<String>,
    selected_category: Option<String>,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pagebuilder/components_content.stpl.html")]
struct PbComponentsContent {
    components: Vec<crate::pagebuilder::PageBuilderComponent>,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pagebuilder/component_form.stpl.html")]
struct PbComponentForm {
    component: Option<crate::pagebuilder::PageBuilderComponent>,
    categories: Vec<String>,
    is_new: bool,
}

#[derive(serde::Deserialize)]
struct PbComponentsSearchQuery {
    category: Option<String>,
    search: Option<String>,
}

#[derive(serde::Deserialize)]
struct PbComponentFormData {
    name: String,
    description: Option<String>,
    category: Option<String>,
    source: String,
}

#[handler]
async fn pb_components_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let components = crate::pagebuilder::PageBuilderComponent::list(None).await?;
    let categories = crate::pagebuilder::PageBuilderComponent::categories().await?;

    let page = PbComponentsListPage {
        components,
        categories,
        selected_category: None,
    };

    let prepared = crate::component::AdminLayout::new("Page Components", page)
        .with_sidebar_active("engine-room-components")
        .with_breadcrumb("Engine Room", Some("/-admin/engine-room"))
        .with_breadcrumb("Components", None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn pb_components_search(
    Query(query): Query<PbComponentsSearchQuery>,
) -> Result<poem::Response, anyhow::Error> {
    let components =
        crate::pagebuilder::PageBuilderComponent::list(query.category.as_deref()).await?;

    // Filter by search if provided
    let components = if let Some(ref search) = query.search {
        let search_lower = search.to_lowercase();
        components
            .into_iter()
            .filter(|c| {
                c.name.to_lowercase().contains(&search_lower)
                    || c.description.to_lowercase().contains(&search_lower)
            })
            .collect()
    } else {
        components
    };

    let content = PbComponentsContent { components };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(content.render_once()?))
}

#[handler]
async fn pb_components_new(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let categories = crate::pagebuilder::PageBuilderComponent::categories().await?;

    let form = PbComponentForm {
        component: None,
        categories,
        is_new: true,
    };

    let prepared = crate::component::AdminLayout::new("New Component", form)
        .with_sidebar_active("engine-room-components")
        .with_breadcrumb("Engine Room", Some("/-admin/engine-room"))
        .with_breadcrumb("Components", Some("/-admin/engine-room/components"))
        .with_breadcrumb("New", None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn pb_components_create(
    Authenticated(user_info): Authenticated,
    poem::web::Form(form_data): poem::web::Form<PbComponentFormData>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::builder::render::CompiledTemplate;
    use crate::peering::LoadStorePeered;

    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    // Validate template syntax before saving
    if let Err(e) = CompiledTemplate::compile(&form_data.source) {
        let error = PbFormError {
            message: e.to_string(),
        };
        return Ok(poem::Response::builder()
            .content_type("text/html")
            .header("HX-Retarget", "body")
            .header("HX-Reswap", "beforeend")
            .body(error.render_once()?));
    }

    let mut component =
        crate::pagebuilder::PageBuilderComponent::new(&form_data.name, &form_data.source);

    if let Some(ref desc) = form_data.description {
        component.description = desc.clone();
    }

    if let Some(ref cat) = form_data.category {
        if !cat.is_empty() {
            component.category = cat.clone();
        }
    }

    component.store().await?;

    // Return redirect with HX-Redirect header for HTMX
    Ok(poem::Response::builder()
        .header("HX-Redirect", "/-admin/engine-room/components")
        .body(""))
}

#[handler]
async fn pb_components_edit(
    Authenticated(user_info): Authenticated,
    Path(id): Path<String>,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    use crate::peering::LoadStorePeered;

    let component_id: ulid::Ulid = id.parse()?;
    let component = crate::pagebuilder::PageBuilderComponent::load(component_id).await?;
    let categories = crate::pagebuilder::PageBuilderComponent::categories().await?;

    let component_name = component.name.clone();

    let form = PbComponentForm {
        component: Some(component),
        categories,
        is_new: false,
    };

    let prepared = crate::component::AdminLayout::new(&format!("Edit: {}", component_name), form)
        .with_sidebar_active("engine-room-components")
        .with_breadcrumb("Engine Room", Some("/-admin/engine-room"))
        .with_breadcrumb("Components", Some("/-admin/engine-room/components"))
        .with_breadcrumb(&component_name, None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn pb_components_update(
    Authenticated(user_info): Authenticated,
    Path(id): Path<String>,
    poem::web::Form(form_data): poem::web::Form<PbComponentFormData>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::builder::render::CompiledTemplate;
    use crate::peering::LoadStorePeered;

    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    // Validate template syntax before saving
    if let Err(e) = CompiledTemplate::compile(&form_data.source) {
        let error = PbFormError {
            message: e.to_string(),
        };
        return Ok(poem::Response::builder()
            .content_type("text/html")
            .header("HX-Retarget", "body")
            .header("HX-Reswap", "beforeend")
            .body(error.render_once()?));
    }

    let component_id: ulid::Ulid = id.parse()?;
    let mut component = crate::pagebuilder::PageBuilderComponent::load(component_id).await?;

    component.name = form_data.name;
    component.description = form_data.description.unwrap_or_default();
    component.set_source(&form_data.source);

    if let Some(ref cat) = form_data.category {
        if !cat.is_empty() {
            component.category = cat.clone();
        }
    }

    component.store().await?;

    // Return redirect with HX-Redirect header for HTMX
    Ok(poem::Response::builder()
        .header("HX-Redirect", "/-admin/engine-room/components")
        .body(""))
}

#[derive(serde::Deserialize)]
struct PbExportForm {
    #[serde(default)]
    ids: Vec<String>,
}

#[handler]
async fn pb_components_export(
    Authenticated(user_info): Authenticated,
    crate::form::QsForm(form): crate::form::QsForm<PbExportForm>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::peering::LoadStorePeered;

    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    if form.ids.is_empty() {
        return Ok(poem::Response::builder()
            .status(poem::http::StatusCode::BAD_REQUEST)
            .content_type("text/plain")
            .body("No components selected for export"));
    }

    let mut components = Vec::with_capacity(form.ids.len());
    for id_str in &form.ids {
        let id: ulid::Ulid = id_str.parse()?;
        components.push(crate::pagebuilder::PageBuilderComponent::load(id).await?);
    }

    let zip_bytes = crate::pagebuilder::library::export_to_zip(&components)?;

    let date = jiff::Timestamp::now().strftime("%Y%m%d-%H%M%S").to_string();
    let filename = format!("components-{date}.zip");

    Ok(poem::Response::builder()
        .content_type("application/zip")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .body(zip_bytes))
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pagebuilder/import_form.stpl.html")]
struct PbImportForm;

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pagebuilder/import_result.stpl.html")]
struct PbImportResult {
    imported: Vec<String>,
    failed: Vec<(String, String)>,
}

#[handler]
async fn pb_components_import_form(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(PbImportForm.render_once()?))
}

#[handler]
async fn pb_components_import(
    Authenticated(user_info): Authenticated,
    mut multipart: poem::web::Multipart,
) -> Result<poem::Response, anyhow::Error> {
    use tokio::io::AsyncReadExt;

    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let mut file_data: Option<Vec<u8>> = None;

    while let Some(field) = multipart.next_field().await? {
        if field.name() == Some("file") {
            let mut data = Vec::new();
            let mut reader = field.into_async_read();
            reader.read_to_end(&mut data).await?;
            file_data = Some(data);
        }
    }

    let bytes = file_data.ok_or_else(|| anyhow::anyhow!("No file uploaded"))?;
    let report = crate::pagebuilder::library::import_from_zip(bytes).await?;

    let result = PbImportResult {
        imported: report.imported,
        failed: report.failed,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(result.render_once()?))
}

#[handler]
async fn pb_components_delete(
    Authenticated(user_info): Authenticated,
    Path(id): Path<String>,
) -> Result<poem::Response, anyhow::Error> {
    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    use crate::peering::LoadStorePeered;

    let component_id: ulid::Ulid = id.parse()?;
    crate::pagebuilder::PageBuilderComponent::delete(component_id).await?;

    // Return redirect with HX-Redirect header for HTMX
    Ok(poem::Response::builder()
        .header("HX-Redirect", "/-admin/engine-room/components")
        .body(""))
}

// ============================================================================
// Page Builder Pages CRUD and Editor
// ============================================================================

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pagebuilder/pages_list.stpl.html")]
struct PbPagesListPage {
    pages: Vec<crate::pagebuilder::PageBuilderPage>,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pagebuilder/pages_content.stpl.html")]
struct PbPagesContent {
    pages: Vec<crate::pagebuilder::PageBuilderPage>,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pagebuilder/page_form.stpl.html")]
struct PbPageForm {
    page: Option<crate::pagebuilder::PageBuilderPage>,
    is_new: bool,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pagebuilder/editor.stpl.html")]
struct PbPageEditor {
    page: crate::pagebuilder::PageBuilderPage,
    components: Vec<crate::pagebuilder::PageBuilderComponent>,
    categories: Vec<String>,
    canvas_html: String,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pagebuilder/editor_error.stpl.html")]
struct PbPageEditorError {
    page: crate::pagebuilder::PageBuilderPage,
    error_message: String,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pagebuilder/source_editor.stpl.html")]
struct PbPageSourceEditor {
    page: crate::pagebuilder::PageBuilderPage,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pagebuilder/canvas_error.stpl.html")]
struct PbCanvasError {
    message: String,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pagebuilder/form_error.stpl.html")]
struct PbFormError {
    message: String,
}

#[derive(serde::Deserialize)]
struct PbPagesSearchQuery {
    search: Option<String>,
}

#[derive(serde::Deserialize)]
struct PbPageFormData {
    name: String,
    description: Option<String>,
    source: Option<String>,
}

#[derive(serde::Deserialize)]
struct PbSlotInsertData {
    component_id: String,
}

#[handler]
async fn pb_pages_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let pages = crate::pagebuilder::PageBuilderPage::list().await?;

    let page = PbPagesListPage { pages };

    let prepared = crate::component::AdminLayout::new("Component Pages", page)
        .with_sidebar_active("component-pages")
        .with_breadcrumb("Component Pages", None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn pb_pages_search(
    Query(query): Query<PbPagesSearchQuery>,
) -> Result<poem::Response, anyhow::Error> {
    let pages = crate::pagebuilder::PageBuilderPage::list().await?;

    // Filter by search if provided
    let pages = if let Some(ref search) = query.search {
        let search_lower = search.to_lowercase();
        pages
            .into_iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&search_lower)
                    || p.description.to_lowercase().contains(&search_lower)
            })
            .collect()
    } else {
        pages
    };

    let content = PbPagesContent { pages };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(content.render_once()?))
}

#[handler]
async fn pb_pages_new(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let form = PbPageForm {
        page: None,
        is_new: true,
    };

    let prepared = crate::component::AdminLayout::new("New Component Page", form)
        .with_sidebar_active("component-pages")
        .with_breadcrumb("Component Pages", Some("/-admin/component-pages"))
        .with_breadcrumb("New", None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn pb_pages_create(
    Authenticated(user_info): Authenticated,
    poem::web::Form(form_data): poem::web::Form<PbPageFormData>,
) -> Result<poem::Response, anyhow::Error> {
    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    use crate::peering::LoadStorePeered;

    // Default source with a main slot
    let source = form_data
        .source
        .unwrap_or_else(|| "{$ slot main $}{$ /slot $}".to_string());

    let mut page = crate::pagebuilder::PageBuilderPage::new(&form_data.name, &source);

    if let Some(ref desc) = form_data.description {
        page.description = desc.clone();
    }

    page.store().await?;

    // Redirect to the editor
    Ok(poem::Response::builder()
        .header(
            "HX-Redirect",
            format!("/-admin/component-pages/{}", page.page_id),
        )
        .body(""))
}

#[handler]
async fn pb_page_editor(
    Authenticated(user_info): Authenticated,
    Path(id): Path<String>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::builder::context::{Context, ContextMode};
    use crate::builder::render::render;
    use crate::peering::LoadStorePeered;

    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let page_id: ulid::Ulid = id.parse()?;
    let page = crate::pagebuilder::PageBuilderPage::load(page_id).await?;

    // Try to render the page in editing mode
    let ctx = Context::new(common::shared::db(), &None, ContextMode::Editing)?;
    let render_result = render(&ctx, &page.source).await;

    match render_result {
        Ok(canvas_html) => {
            // Render succeeded - show normal editor
            let components = crate::pagebuilder::PageBuilderComponent::list(None).await?;
            let categories = crate::pagebuilder::PageBuilderComponent::categories().await?;

            let editor = PbPageEditor {
                page,
                components,
                categories,
                canvas_html,
            };

            let prepared = crate::component::AdminLayout::new("Page Editor", editor)
                .with_sidebar_active("component-pages")
                .with_breadcrumb("Component Pages", Some("/-admin/component-pages"))
                .with_breadcrumb("Editor", None)
                .with_quill()
                .prepare(Some(user_info))
                .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(prepared.render_once()?))
        }
        Err(e) => {
            // Render failed - show error editor to fix the source
            let error_editor = PbPageEditorError {
                page,
                error_message: e.to_string(),
            };

            let prepared = crate::component::AdminLayout::new("Page Editor - Error", error_editor)
                .with_sidebar_active("component-pages")
                .with_breadcrumb("Component Pages", Some("/-admin/component-pages"))
                .with_breadcrumb("Editor (Error)", None)
                .with_quill()
                .prepare(Some(user_info))
                .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(prepared.render_once()?))
        }
    }
}

#[handler]
async fn pb_page_edit_details(
    Authenticated(user_info): Authenticated,
    Path(id): Path<String>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::peering::LoadStorePeered;

    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let page_id: ulid::Ulid = id.parse()?;
    let page = crate::pagebuilder::PageBuilderPage::load(page_id).await?;

    let form = PbPageForm {
        page: Some(page),
        is_new: false,
    };

    let prepared = crate::component::AdminLayout::new("Edit Page", form)
        .with_sidebar_active("component-pages")
        .with_breadcrumb("Component Pages", Some("/-admin/component-pages"))
        .with_breadcrumb("Edit", None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn pb_page_update(
    Authenticated(user_info): Authenticated,
    Path(id): Path<String>,
    poem::web::Form(form_data): poem::web::Form<PbPageFormData>,
) -> Result<poem::Response, anyhow::Error> {
    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    use crate::peering::LoadStorePeered;

    let page_id: ulid::Ulid = id.parse()?;
    let mut page = crate::pagebuilder::PageBuilderPage::load(page_id).await?;

    page.name = form_data.name;
    page.description = form_data.description.unwrap_or_default();

    if let Some(source) = form_data.source {
        page.set_source(&source);
    }

    page.store().await?;

    Ok(poem::Response::builder()
        .header(
            "HX-Redirect",
            format!("/-admin/component-pages/{}", page_id),
        )
        .body(""))
}

#[handler]
async fn pb_page_source_editor(
    Authenticated(user_info): Authenticated,
    Path(id): Path<String>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::peering::LoadStorePeered;

    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let page_id: ulid::Ulid = id.parse()?;
    let page = crate::pagebuilder::PageBuilderPage::load(page_id).await?;

    let editor = PbPageSourceEditor { page };

    let prepared = crate::component::AdminLayout::new("Edit Page Source", editor)
        .with_sidebar_active("component-pages")
        .with_breadcrumb("Component Pages", Some("/-admin/component-pages"))
        .with_breadcrumb("Source", None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[derive(serde::Deserialize)]
struct PbPageSourceData {
    source: String,
}

#[handler]
async fn pb_page_update_source(
    Authenticated(user_info): Authenticated,
    Path(id): Path<String>,
    poem::web::Form(form_data): poem::web::Form<PbPageSourceData>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::builder::render::CompiledTemplate;
    use crate::peering::LoadStorePeered;

    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let page_id: ulid::Ulid = id.parse()?;
    let mut page = crate::pagebuilder::PageBuilderPage::load(page_id).await?;

    // Validate syntax before saving
    if let Err(e) = CompiledTemplate::compile(&form_data.source) {
        let error_editor = PbPageEditorError {
            page,
            error_message: e.to_string(),
        };
        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(error_editor.render_once()?));
    }

    page.set_source(&form_data.source);
    page.store().await?;

    Ok(poem::Response::builder()
        .header(
            "HX-Redirect",
            format!("/-admin/component-pages/{}", page_id),
        )
        .body(""))
}

#[handler]
async fn pb_page_delete(
    Authenticated(user_info): Authenticated,
    Path(id): Path<String>,
) -> Result<poem::Response, anyhow::Error> {
    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    use crate::peering::LoadStorePeered;

    let page_id: ulid::Ulid = id.parse()?;
    crate::pagebuilder::PageBuilderPage::delete(page_id).await?;

    Ok(poem::Response::builder()
        .header("HX-Redirect", "/-admin/component-pages")
        .body(""))
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/content.stpl.html")]
struct PbPreviewPage {
    head: <crate::component::Head as crate::component::Component>::Prepared,
    layout: <crate::component::StandardLayout<PbPreviewContent> as crate::component::Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/content_page_content.stpl.html")]
struct PbPreviewContent {
    content: crate::misc::Literal<'static>,
}

#[derive(serde::Deserialize)]
struct PbPreviewQuery {
    /// "user" for logged-in preview, "guest" for logged-out preview
    #[serde(rename = "as", default)]
    preview_as: String,
}

#[handler]
async fn pb_page_preview(
    Authenticated(user_info): Authenticated,
    Path(id): Path<String>,
    Query(query): Query<PbPreviewQuery>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::builder::context::{Context, ContextMode};
    use crate::builder::render::render;
    use crate::component::{Component, Head, StandardLayout};
    use crate::misc::Literal;
    use crate::peering::LoadStorePeered;

    // Must be authenticated to access preview
    let admin_user = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let page_id: ulid::Ulid = id.parse()?;
    let page = crate::pagebuilder::PageBuilderPage::load(page_id).await?;

    // Determine which user context to use for preview
    let preview_user = if query.preview_as == "guest" {
        None
    } else {
        Some(admin_user.clone())
    };

    // Render in presenting mode for preview
    let ctx = Context::new(common::shared::db(), &preview_user, ContextMode::Presenting)?;
    let html = render(&ctx, &page.source).await?;

    // Wrap in standard layout like entity pages do
    let content_wrapper = PbPreviewContent {
        content: Literal::new(Box::leak(html.into_boxed_str())),
    };

    let layout = StandardLayout::new("fake-path", content_wrapper)
        .prepare(preview_user)
        .await?;

    let page = PbPreviewPage {
        head: Head::default().prepare(None).await?,
        layout,
    };

    Ok(page.into_response(None).await?)
}

#[handler]
async fn pb_slot_insert(
    Authenticated(user_info): Authenticated,
    Path((id, slot_name)): Path<(String, String)>,
    poem::web::Form(data): poem::web::Form<PbSlotInsertData>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::builder::context::{Context, ContextMode};
    use crate::builder::render::{CompiledTemplate, render};
    use crate::peering::LoadStorePeered;

    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let page_id: ulid::Ulid = id.parse()?;
    let component_id: ulid::Ulid = data.component_id.parse()?;

    let mut page = crate::pagebuilder::PageBuilderPage::load(page_id).await?;
    let component = crate::pagebuilder::PageBuilderComponent::load(component_id).await?;

    // Extract default param values from the component template
    let default_params: Vec<(String, String)> =
        if let Ok(compiled) = CompiledTemplate::compile(&component.source) {
            compiled
                .extract_params()
                .into_iter()
                .filter_map(|p| p.default.map(|v| (p.name, v.to_string())))
                .collect()
        } else {
            vec![]
        };

    // Insert the component into the slot with default params
    let new_source = crate::pagebuilder::source::insert_component_into_slot(
        &page.source,
        &slot_name,
        &component,
        &default_params,
        common::shared::db(),
    )
    .await?;

    // Try to render the new source before saving
    let ctx = Context::new(common::shared::db(), &None, ContextMode::Editing)?;
    let canvas_html = match render(&ctx, &new_source).await {
        Ok(html) => html,
        Err(e) => {
            // Render failed - return error without saving
            let error = PbCanvasError {
                message: format!(
                    "Component '{}' has invalid template syntax: {}",
                    component.name, e
                ),
            };
            return Ok(poem::Response::builder()
                .content_type("text/html")
                .body(error.render_once()?));
        }
    };

    // Rendering succeeded, now save the page
    page.set_source(&new_source);
    page.store().await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(canvas_html))
}

#[handler]
async fn pb_slot_clear(
    Authenticated(user_info): Authenticated,
    Path((id, slot_name)): Path<(String, String)>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::builder::context::{Context, ContextMode};
    use crate::builder::render::render;
    use crate::peering::LoadStorePeered;

    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let page_id: ulid::Ulid = id.parse()?;

    let mut page = crate::pagebuilder::PageBuilderPage::load(page_id).await?;

    // Clear the slot
    let new_source = crate::pagebuilder::source::clear_slot(&page.source, &slot_name)?;

    page.set_source(&new_source);
    page.store().await?;

    // Re-render the canvas in editing mode
    let ctx = Context::new(common::shared::db(), &None, ContextMode::Editing)?;
    let canvas_html = render(&ctx, &page.source).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(canvas_html))
}

#[handler]
async fn pb_slot_move(
    Authenticated(user_info): Authenticated,
    Path((id, target_slot, source_slot)): Path<(String, String, String)>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::builder::context::{Context, ContextMode};
    use crate::builder::render::render;
    use crate::pagebuilder::source::move_slot_content;
    use crate::peering::LoadStorePeered;

    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let page_id: ulid::Ulid = id.parse()?;
    let mut page = crate::pagebuilder::PageBuilderPage::load(page_id).await?;

    // Move slot content preserving all nested content
    let new_source = move_slot_content(&page.source, &source_slot, &target_slot)?;

    page.set_source(&new_source);
    page.store().await?;

    // Re-render the canvas in editing mode
    let ctx = Context::new(common::shared::db(), &None, ContextMode::Editing)?;
    let canvas_html = render(&ctx, &page.source).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(canvas_html))
}

#[handler]
async fn pb_slot_copy(
    Authenticated(user_info): Authenticated,
    Path((id, target_slot, source_slot)): Path<(String, String, String)>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::builder::context::{Context, ContextMode};
    use crate::builder::render::render;
    use crate::pagebuilder::source::copy_slot_content;
    use crate::peering::LoadStorePeered;

    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let page_id: ulid::Ulid = id.parse()?;
    let mut page = crate::pagebuilder::PageBuilderPage::load(page_id).await?;

    // Copy slot content preserving all nested content
    let new_source = copy_slot_content(&page.source, &source_slot, &target_slot)?;

    page.set_source(&new_source);
    page.store().await?;

    // Re-render the canvas in editing mode
    let ctx = Context::new(common::shared::db(), &None, ContextMode::Editing)?;
    let canvas_html = render(&ctx, &page.source).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(canvas_html))
}

/// JSON response for slot extraction (used by staging panel)
#[derive(serde::Serialize)]
struct PbSlotExtractResponse {
    component_id: Option<String>,
    params: Vec<(String, String)>,
    body: String,
    label: String,
}

#[handler]
async fn pb_slot_extract(
    Authenticated(user_info): Authenticated,
    Path((id, slot_name)): Path<(String, String)>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::pagebuilder::source::extract_slot_source;
    use crate::peering::LoadStorePeered;

    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let page_id: ulid::Ulid = id.parse()?;
    let page = crate::pagebuilder::PageBuilderPage::load(page_id).await?;

    let (component_id, params, body) = extract_slot_source(&page.source, &slot_name)?;

    // Use the component name as the label, falling back to slot name
    let label = if let Some(cid) = component_id {
        match crate::pagebuilder::PageBuilderComponent::load(cid).await {
            Ok(component) => component.name,
            Err(_) => slot_name
                .split('.')
                .last()
                .unwrap_or(&slot_name)
                .to_string(),
        }
    } else {
        slot_name
            .split('.')
            .last()
            .unwrap_or(&slot_name)
            .to_string()
    };

    let response = PbSlotExtractResponse {
        component_id: component_id.map(|id| id.to_string()),
        params,
        body,
        label,
    };

    Ok(poem::Response::builder()
        .content_type("application/json")
        .body(serde_json::to_string(&response)?))
}

#[derive(serde::Deserialize)]
struct PbSlotInsertRawData {
    component_id: Option<String>,
    #[serde(default)]
    params: Vec<(String, String)>,
    body: String,
}

#[handler]
async fn pb_slot_insert_raw(
    Authenticated(user_info): Authenticated,
    Path((id, slot_name)): Path<(String, String)>,
    poem::web::Json(data): poem::web::Json<PbSlotInsertRawData>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::builder::context::{Context, ContextMode};
    use crate::builder::render::render;
    use crate::pagebuilder::source::insert_raw_source;
    use crate::peering::LoadStorePeered;

    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let page_id: ulid::Ulid = id.parse()?;
    let mut page = crate::pagebuilder::PageBuilderPage::load(page_id).await?;

    let component_id: Option<ulid::Ulid> =
        data.component_id.as_ref().map(|s| s.parse()).transpose()?;

    let new_source = insert_raw_source(
        &page.source,
        &slot_name,
        component_id.as_ref(),
        &data.params,
        &data.body,
    )?;

    page.set_source(&new_source);
    page.store().await?;

    // Re-render the canvas in editing mode
    let ctx = Context::new(common::shared::db(), &None, ContextMode::Editing)?;
    let canvas_html = render(&ctx, &page.source).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(canvas_html))
}

/// A param definition with its current value for the slot params form
struct SlotParamField {
    /// The param definition (name, label, description, default)
    def: crate::builder::render::ParamDef,
    /// The current value (from slot params or default)
    current_value: String,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "admin/pagebuilder/slot_params_form.stpl.html")]
struct PbSlotParamsForm {
    page_id: ulid::Ulid,
    slot_path: String,
    component: crate::pagebuilder::PageBuilderComponent,
    params: Vec<SlotParamField>,
}

#[handler]
async fn pb_slot_params_form(
    Authenticated(user_info): Authenticated,
    Path((id, slot_path)): Path<(String, String)>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::builder::render::CompiledTemplate;
    use crate::peering::LoadStorePeered;

    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let page_id: ulid::Ulid = id.parse()?;
    let page = crate::pagebuilder::PageBuilderPage::load(page_id).await?;

    // Find the slot by path
    let slots = crate::pagebuilder::source::extract_slots(&page.source)?;
    let slot = slots
        .iter()
        .find(|s| s.path == slot_path)
        .ok_or_else(|| anyhow::anyhow!("Slot not found"))?;

    let component_id = slot
        .component_id
        .ok_or_else(|| anyhow::anyhow!("Slot has no component"))?;
    let component = crate::pagebuilder::PageBuilderComponent::load(component_id).await?;

    // Extract param definitions from component template
    let compiled = CompiledTemplate::compile(&component.source)?;
    let param_defs = compiled.extract_params();

    // Build current params lookup
    let current_params: std::collections::HashMap<_, _> = slot.params.iter().cloned().collect();

    // Build param fields with current values or defaults
    let params: Vec<SlotParamField> = param_defs
        .into_iter()
        .map(|def| {
            let current_value = current_params.get(&def.name).cloned().unwrap_or_else(|| {
                def.default
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            });
            SlotParamField { def, current_value }
        })
        .collect();

    let form = PbSlotParamsForm {
        page_id,
        slot_path,
        component,
        params,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}

#[derive(serde::Deserialize)]
struct PbSlotParamsData {
    #[serde(flatten)]
    params: std::collections::HashMap<String, String>,
}

#[handler]
async fn pb_slot_update_params(
    Authenticated(user_info): Authenticated,
    Path((id, slot_path)): Path<(String, String)>,
    poem::web::Form(data): poem::web::Form<PbSlotParamsData>,
) -> Result<poem::Response, anyhow::Error> {
    use crate::builder::context::{Context, ContextMode};
    use crate::builder::render::render;
    use crate::peering::LoadStorePeered;

    let _user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let page_id: ulid::Ulid = id.parse()?;
    let page = crate::pagebuilder::PageBuilderPage::load(page_id).await?;

    // Find the slot by path
    let slots = crate::pagebuilder::source::extract_slots(&page.source)?;
    let slot = slots
        .iter()
        .find(|s| s.path == slot_path)
        .ok_or_else(|| anyhow::anyhow!("Slot not found"))?;

    let component_id = slot
        .component_id
        .ok_or_else(|| anyhow::anyhow!("Slot has no component"))?;
    let component = crate::pagebuilder::PageBuilderComponent::load(component_id).await?;

    // Convert params HashMap to Vec
    let params: Vec<(String, String)> = data
        .params
        .into_iter()
        .filter(|(k, _)| k.starts_with("param_"))
        .map(|(k, v)| (k.strip_prefix("param_").unwrap().to_string(), v))
        .collect();

    // Update the slot with new params
    let new_source = crate::pagebuilder::source::update_slot_params(
        &page.source,
        &slot_path,
        &component,
        &params,
        common::shared::db(),
    )
    .await?;

    let mut page = page;
    page.set_source(&new_source);
    page.store().await?;

    // Re-render the canvas in editing mode
    let ctx = Context::new(common::shared::db(), &None, ContextMode::Editing)?;
    let canvas_html = render(&ctx, &page.source).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(canvas_html))
}

// ============================================================================
// Routes
// ============================================================================

/// Routes for /-admin/engine-room/*
pub fn engine_room_routes() -> Route {
    Route::new()
        .at("/", get(engine_room))
        .at("/sync-page-managers", post(engine_room_sync_page_managers))
        .at("/components", get(pb_components_list))
        .at("/components/search", get(pb_components_search))
        .at("/components/export", post(pb_components_export))
        .at(
            "/components/import",
            get(pb_components_import_form).post(pb_components_import),
        )
        .at(
            "/components/new",
            get(pb_components_new).post(pb_components_create),
        )
        .at(
            "/components/:id",
            get(pb_components_edit)
                .post(pb_components_update)
                .delete(pb_components_delete),
        )
}

/// Routes for /-admin/component-pages/*
pub fn pages_routes() -> Route {
    Route::new()
        .at("/", get(pb_pages_list))
        .at("/search", get(pb_pages_search))
        .at("/new", get(pb_pages_new).post(pb_pages_create))
        .at("/:id", get(pb_page_editor).delete(pb_page_delete))
        .at("/:id/edit", get(pb_page_edit_details))
        .at("/:id/update", post(pb_page_update))
        .at(
            "/:id/source",
            get(pb_page_source_editor).post(pb_page_update_source),
        )
        .at("/:id/preview", get(pb_page_preview))
        .at(
            "/:id/slot/:slot_name",
            post(pb_slot_insert).delete(pb_slot_clear),
        )
        .at(
            "/:id/slot/:slot_name/params",
            get(pb_slot_params_form).put(pb_slot_update_params),
        )
        .at(
            "/:id/slot/:target_slot/move-from/:source_slot",
            post(pb_slot_move),
        )
        .at(
            "/:id/slot/:target_slot/copy-from/:source_slot",
            post(pb_slot_copy),
        )
        .at("/:id/slot/:slot_name/extract", get(pb_slot_extract))
        .at("/:id/slot/:slot_name/insert-raw", post(pb_slot_insert_raw))
}
