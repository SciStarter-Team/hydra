use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::staff_pick::StaffPick;
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
#[template(path = "admin/staff_picks.stpl.html")]
struct StaffPicksPage {
    picks_content: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/staff_picks_content.stpl.html")]
struct StaffPicksContentTemplate {
    picks: Vec<StaffPick>,
    search_query: Option<String>,
    page: usize,
    per_page: usize,
    total_count: usize,
}

#[derive(TemplateOnce)]
#[template(path = "admin/edit_staff_pick_content.stpl.html")]
struct EditStaffPickContent {
    pick: StaffPick,
    error_message: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/modals/new_staff_pick.stpl.html")]
struct NewStaffPickForm {
    error_message: Option<String>,
}

struct StaffPickRow {
    id: String,
    project_slug: String,
    label: String,
    description: String,
    location: String,
    acts_per_hour: String,
    picker_name: String,
    picker_image: String,
    picker_quote: String,
    priority: i32,
    begin_date: String,
    end_date: String,
}

impl TryFrom<StaffPickRow> for StaffPick {
    type Error = anyhow::Error;

    fn try_from(row: StaffPickRow) -> Result<Self, Self::Error> {
        Ok(StaffPick {
            id: row.id.parse()?,
            project_slug: row.project_slug,
            label: row.label,
            description: row.description,
            location: row.location,
            acts_per_hour: row.acts_per_hour,
            picker_name: row.picker_name,
            picker_image: row.picker_image,
            picker_quote: row.picker_quote,
            priority: row.priority,
            begin_date: row.begin_date,
            end_date: row.end_date,
        })
    }
}

// Query and form types
#[derive(Deserialize)]
struct SearchQuery {
    search: Option<String>,
    page: Option<usize>,
}

#[derive(Deserialize)]
struct StaffPickForm {
    project_slug: String,
    label: String,
    description: Option<String>,
    location: Option<String>,
    acts_per_hour: Option<String>,
    picker_name: String,
    picker_image: Option<String>,
    picker_quote: Option<String>,
    priority: Option<i32>,
    begin_date: String,
    end_date: String,
}

// Helper functions
async fn search_impl(query: &SearchQuery) -> Result<String, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page: i64 = 25;
    let offset = (page - 1) as i64 * per_page;

    let rows = if let Some(ref search_term) = query.search {
        let pattern = format!("%{}%", search_term);
        sqlx::query_as!(
            StaffPickRow,
            r#"SELECT id, project_slug, label, description, location,
                      acts_per_hour, picker_name, picker_image, picker_quote,
                      priority as "priority: i32", begin_date, end_date
               FROM csm_staff_picks
               WHERE label LIKE ?1 OR picker_name LIKE ?1 OR project_slug LIKE ?1
               ORDER BY priority DESC, begin_date DESC
               LIMIT ?2 OFFSET ?3"#,
            pattern,
            per_page,
            offset
        )
        .fetch_all(db())
        .await?
    } else {
        sqlx::query_as!(
            StaffPickRow,
            r#"SELECT id, project_slug, label, description, location,
                      acts_per_hour, picker_name, picker_image, picker_quote,
                      priority as "priority: i32", begin_date, end_date
               FROM csm_staff_picks
               ORDER BY priority DESC, begin_date DESC
               LIMIT ?1 OFFSET ?2"#,
            per_page,
            offset
        )
        .fetch_all(db())
        .await?
    };

    let picks = rows
        .into_iter()
        .map(StaffPick::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    let total_count: i64 = if let Some(ref search_term) = query.search {
        let pattern = format!("%{}%", search_term);
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM csm_staff_picks WHERE label LIKE ? OR picker_name LIKE ? OR project_slug LIKE ?",
            pattern,
            pattern,
            pattern
        )
        .fetch_one(db())
        .await?
    } else {
        sqlx::query_scalar!("SELECT COUNT(*) FROM csm_staff_picks")
            .fetch_one(db())
            .await?
    };

    let template = StaffPicksContentTemplate {
        picks,
        search_query: query.search.clone(),
        page,
        per_page: per_page as usize,
        total_count: total_count as usize,
    };

    Ok(template.render_once()?)
}

// Handlers
#[handler]
async fn list(Authenticated(user_info): Authenticated) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let search_results = search_impl(&SearchQuery {
        search: None,
        page: Some(1),
    })
    .await?;

    let content = format!(r#"<div id="staff-picks-content">{}</div>"#, search_results);

    let page = StaffPicksPage {
        picks_content: content,
    };

    let prepared = AdminLayout::new("CSM Staff Picks", page)
        .with_sidebar_active("staff-picks")
        .with_breadcrumb("Staff Picks", None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn search(
    Query(query): Query<SearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = search_impl(&query).await?;
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(content))
}

#[handler]
async fn new_form(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let form = NewStaffPickForm {
        error_message: None,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}

#[handler]
async fn create(
    Form(form): Form<StaffPickForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    match (async || -> Result<Ulid, anyhow::Error> {
        if form.project_slug.trim().is_empty() {
            return Err(anyhow::anyhow!("Project slug is required"));
        }
        if form.picker_name.trim().is_empty() {
            return Err(anyhow::anyhow!("Picker name is required"));
        }

        let pick = StaffPick {
            id: Ulid::new(),
            project_slug: form.project_slug.clone(),
            label: form.label,
            description: form.description.unwrap_or_default(),
            location: form.location.unwrap_or_default(),
            acts_per_hour: form.acts_per_hour.unwrap_or_default(),
            picker_name: form.picker_name,
            picker_image: form.picker_image.unwrap_or_default(),
            picker_quote: form.picker_quote.unwrap_or_default(),
            priority: form.priority.unwrap_or(0),
            begin_date: form.begin_date,
            end_date: form.end_date,
        };

        let id = pick.id;
        pick.store().await?;

        if let Some(user_info) = &user_info {
            common::service::log_activity(
                None,
                "staff-pick-create",
                user_info.user_id,
                id,
                format!("slug={}", form.project_slug),
            )
            .await?;
        }

        Ok(id)
    })()
    .await
    {
        Ok(_id) => {
            let content = search_impl(&SearchQuery {
                search: None,
                page: Some(1),
            })
            .await?;

            let html = format!(
                r#"<div id="staff-picks-content" hx-swap-oob="innerHTML">{}</div>"#,
                content
            );

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(html))
        }
        Err(e) => {
            let form = NewStaffPickForm {
                error_message: Some(e.to_string()),
            };

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(form.render_once()?))
        }
    }
}

#[handler]
async fn edit(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let pick_id: Ulid = id_str.parse()?;
    let pick = StaffPick::load(pick_id).await?;

    let content = EditStaffPickContent {
        pick,
        error_message: None,
    };

    let layout = AdminLayout::new("Edit Staff Pick", content)
        .with_breadcrumb("Staff Picks", Some("/-admin/staff-picks"))
        .with_breadcrumb("Edit", None)
        .with_sidebar_active("staff-picks");

    let prepared = layout.prepare(user_info).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn update(
    Path(id_str): Path<String>,
    Form(form): Form<StaffPickForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let pick_id: Ulid = id_str.parse()?;

    match (async || -> Result<(), anyhow::Error> {
        let pick = StaffPick {
            id: pick_id,
            project_slug: form.project_slug.clone(),
            label: form.label,
            description: form.description.unwrap_or_default(),
            location: form.location.unwrap_or_default(),
            acts_per_hour: form.acts_per_hour.unwrap_or_default(),
            picker_name: form.picker_name,
            picker_image: form.picker_image.unwrap_or_default(),
            picker_quote: form.picker_quote.unwrap_or_default(),
            priority: form.priority.unwrap_or(0),
            begin_date: form.begin_date,
            end_date: form.end_date,
        };

        pick.store().await?;

        if let Some(user_info) = &user_info {
            common::service::log_activity(
                None,
                "staff-pick-update",
                user_info.user_id,
                pick_id,
                format!("slug={}", form.project_slug),
            )
            .await?;
        }

        Ok(())
    })()
    .await
    {
        Ok(()) => Ok(poem::Response::builder()
            .status(poem::http::StatusCode::SEE_OTHER)
            .header("Location", "/-admin/staff-picks")
            .finish()),
        Err(e) => {
            let pick = StaffPick::load(pick_id).await?;
            let content = EditStaffPickContent {
                pick,
                error_message: Some(e.to_string()),
            };

            let layout = AdminLayout::new("Edit Staff Pick", content)
                .with_breadcrumb("Staff Picks", Some("/-admin/staff-picks"))
                .with_breadcrumb("Edit", None)
                .with_sidebar_active("staff-picks")
                .prepare(user_info)
                .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(layout.render_once()?))
        }
    }
}

#[handler]
async fn delete(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let pick_id: Ulid = id_str.parse()?;

    StaffPick::delete(pick_id).await?;

    if let Some(user_info) = &user_info {
        common::service::log_activity(None, "staff-pick-delete", user_info.user_id, pick_id, None)
            .await?;
    }

    Ok(poem::Response::builder().content_type("text/html").body(""))
}

// Routes export
pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(list).post(create))
        .at("/search", get(search))
        .at("/new", get(new_form))
        .at("/:id", get(edit).post(update).delete(delete))
}
