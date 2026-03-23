// Schedule management for admin

use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::schedule::{Schedule, ScheduleType, TargetType};
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
#[template(path = "admin/schedules.stpl.html")]
struct SchedulesPage {
    schedules_content: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/schedules_content.stpl.html")]
struct SchedulesContentTemplate {
    items: Vec<Schedule>,
    search_query: Option<String>,
    page: usize,
    per_page: usize,
    total_count: usize,
}

struct ScheduleRow {
    schedule_id: String,
    schedule_type: String,
    target_type: Option<String>,
    target: Option<String>,
    featured_entity: Option<String>,
    title: String,
    content: Option<String>,
    url: Option<String>,
    start_date: String,
    end_date: String,
    active: i64,
}

#[derive(TemplateOnce)]
#[template(path = "admin/modals/new_schedule.stpl.html")]
struct NewScheduleForm {
    error_message: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/edit_schedule_content.stpl.html")]
struct EditScheduleContent {
    schedule: Schedule,
    error_message: Option<String>,
}

// Query and form types
#[derive(Deserialize)]
struct SchedulesSearchQuery {
    search: Option<String>,
    page: Option<usize>,
}

#[derive(Deserialize)]
struct CreateScheduleForm {
    title: String,
    schedule_type: String,
    target_type: Option<String>,
    target: Option<String>,
    featured_entity: Option<String>,
    content: Option<String>,
    url: Option<String>,
    start_date: String,
    end_date: String,
    active: Option<String>,
}

#[derive(Deserialize)]
struct UpdateScheduleForm {
    title: String,
    schedule_type: String,
    target_type: Option<String>,
    target: Option<String>,
    featured_entity: Option<String>,
    content: Option<String>,
    url: Option<String>,
    start_date: String,
    end_date: String,
    active: Option<String>,
}

// Helper functions
async fn schedules_search_impl(query: &SchedulesSearchQuery) -> Result<String, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page: i64 = 25;
    let offset = (page - 1) as i64 * per_page;

    let items_raw = if let Some(ref search) = query.search {
        let pattern = format!("%{}%", search);
        sqlx::query_as!(
            ScheduleRow,
            r#"SELECT schedule_id,
                      schedule_type,
                      target_type,
                      target,
                      featured_entity,
                      title,
                      content,
                      url,
                      start_date,
                      end_date,
                      active
               FROM schedules
               WHERE title LIKE ?1
               ORDER BY start_date DESC
               LIMIT ?2 OFFSET ?3"#,
            pattern,
            per_page,
            offset
        )
        .fetch_all(db())
        .await?
    } else {
        sqlx::query_as!(
            ScheduleRow,
            r#"SELECT schedule_id,
                      schedule_type,
                      target_type,
                      target,
                      featured_entity,
                      title,
                      content,
                      url,
                      start_date,
                      end_date,
                      active
               FROM schedules
               ORDER BY start_date DESC
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
            Ok(Schedule {
                schedule_id: row.schedule_id.parse()?,
                schedule_type: row.schedule_type.parse()?,
                target_type: row.target_type.as_deref().map(|s| s.parse()).transpose()?,
                target: row.target,
                featured_entity: row.featured_entity,
                title: row.title,
                content: row.content,
                url: row.url,
                start_date: row.start_date,
                end_date: row.end_date,
                active: row.active != 0,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let total_count: i64 = if let Some(ref search) = query.search {
        let pattern = format!("%{}%", search);
        sqlx::query_scalar!("SELECT COUNT(*) FROM schedules WHERE title LIKE ?", pattern)
            .fetch_one(db())
            .await?
    } else {
        sqlx::query_scalar!("SELECT COUNT(*) FROM schedules")
            .fetch_one(db())
            .await?
    };

    let template = SchedulesContentTemplate {
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
async fn schedules_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let search_results = schedules_search_impl(&SchedulesSearchQuery {
        search: None,
        page: Some(1),
    })
    .await?;

    let content = format!(r#"<div id="schedules-content">{}</div>"#, search_results);

    let page = SchedulesPage {
        schedules_content: content,
    };

    let prepared = AdminLayout::new("Scheduling", page)
        .with_sidebar_active("schedules")
        .with_breadcrumb("Scheduling", None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn schedules_search(
    Query(query): Query<SchedulesSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = schedules_search_impl(&query).await?;
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(content))
}

#[handler]
async fn schedules_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let form = NewScheduleForm {
        error_message: None,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}

#[handler]
async fn schedules_create(
    Form(form): Form<CreateScheduleForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    match (async || -> Result<Ulid, anyhow::Error> {
        if form.title.trim().is_empty() {
            return Err(anyhow::anyhow!("Title is required"));
        }

        let schedule_type: ScheduleType = form.schedule_type.parse()?;
        let target_type: Option<TargetType> = form
            .target_type
            .as_ref()
            .and_then(|t| {
                if t.is_empty() {
                    None
                } else {
                    Some(t.parse().ok())
                }
            })
            .flatten();

        let target = form
            .target
            .as_ref()
            .and_then(|t| if t.is_empty() { None } else { Some(t.clone()) });

        let featured_entity = form
            .featured_entity
            .as_ref()
            .and_then(|t| if t.is_empty() { None } else { Some(t.clone()) });

        let mut schedule = Schedule::new(
            schedule_type,
            target_type,
            target,
            featured_entity,
            form.title.clone(),
            form.content,
            form.url,
            form.start_date,
            form.end_date,
        );

        // Override active status if checkbox was unchecked
        if form.active.is_none() {
            schedule.active = false;
        }

        let id = schedule.schedule_id;
        schedule.store().await?;

        // Log the activity
        if let Some(user_info) = &user_info {
            common::service::log_activity(
                None,
                "schedule-create",
                user_info.user_id,
                id,
                format!("type={} title={}", schedule_type, form.title),
            )
            .await?;
        }

        Ok(id)
    })()
    .await
    {
        Ok(_id) => {
            // Return OOB swap with refreshed list
            let content = schedules_search_impl(&SchedulesSearchQuery {
                search: None,
                page: Some(1),
            })
            .await?;

            let html = format!(
                r#"<div id="schedules-content" hx-swap-oob="innerHTML">{}</div>"#,
                content
            );

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(html))
        }
        Err(e) => {
            let form = NewScheduleForm {
                error_message: Some(e.to_string()),
            };

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(form.render_once()?))
        }
    }
}

#[handler]
async fn schedules_edit(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let schedule_id: Ulid = id_str.parse()?;
    let schedule = Schedule::load(schedule_id).await?;

    let content = EditScheduleContent {
        schedule,
        error_message: None,
    };

    let layout = AdminLayout::new("Edit Schedule", content)
        .with_breadcrumb("Schedules", Some("/-admin/schedules"))
        .with_breadcrumb("Edit", None)
        .with_sidebar_active("schedules")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(layout.render_once()?))
}

#[handler]
async fn schedules_update(
    Path(id_str): Path<String>,
    Form(form): Form<UpdateScheduleForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let schedule_id: Ulid = id_str.parse()?;

    match (async || -> Result<(), anyhow::Error> {
        if form.title.trim().is_empty() {
            return Err(anyhow::anyhow!("Title is required"));
        }

        let schedule_type: ScheduleType = form.schedule_type.parse()?;
        let target_type: Option<TargetType> = form
            .target_type
            .as_ref()
            .and_then(|t| {
                if t.is_empty() {
                    None
                } else {
                    Some(t.parse().ok())
                }
            })
            .flatten();

        let target = form
            .target
            .as_ref()
            .and_then(|t| if t.is_empty() { None } else { Some(t.clone()) });

        let featured_entity = form
            .featured_entity
            .as_ref()
            .and_then(|t| if t.is_empty() { None } else { Some(t.clone()) });

        let active = form.active.is_some();

        let schedule = Schedule {
            schedule_id,
            schedule_type,
            target_type,
            target,
            featured_entity,
            title: form.title.clone(),
            content: form.content,
            url: form.url,
            start_date: form.start_date,
            end_date: form.end_date,
            active,
        };

        schedule.store().await?;

        // Log the activity
        if let Some(user_info) = &user_info {
            common::service::log_activity(
                None,
                "schedule-update",
                user_info.user_id,
                schedule_id,
                format!("type={} title={}", schedule_type, form.title),
            )
            .await?;
        }

        Ok(())
    })()
    .await
    {
        Ok(()) => {
            // Redirect back to schedules list
            Ok(poem::Response::builder()
                .status(poem::http::StatusCode::SEE_OTHER)
                .header("Location", "/-admin/schedules")
                .finish())
        }
        Err(e) => {
            // Reload the page with error message
            let schedule = Schedule::load(schedule_id).await?;
            let content = EditScheduleContent {
                schedule,
                error_message: Some(e.to_string()),
            };

            let layout = AdminLayout::new("Edit Schedule", content)
                .with_breadcrumb("Schedules", Some("/-admin/schedules"))
                .with_breadcrumb("Edit", None)
                .with_sidebar_active("schedules")
                .prepare(user_info)
                .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(layout.render_once()?))
        }
    }
}

#[handler]
async fn schedules_delete(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let schedule_id: Ulid = id_str.parse()?;

    // Load schedule before deletion for logging
    let schedule = Schedule::load(schedule_id).await.ok();

    // Use LoadStorePeered delete to properly notify peers
    Schedule::delete(schedule_id).await?;

    // Log the activity
    if let Some(user_info) = &user_info {
        let extra = schedule
            .as_ref()
            .map(|s| format!("type={} title={}", s.schedule_type, s.title));
        common::service::log_activity(
            None,
            "schedule-delete",
            user_info.user_id,
            schedule_id,
            extra,
        )
        .await?;
    }

    // Check if there are any schedules left
    let exists: bool =
        sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM schedules) as "exists: bool""#)
            .fetch_one(db())
            .await?;

    let html = if !exists {
        // If no items left, return OOB swap with empty state
        let content = schedules_search_impl(&SchedulesSearchQuery {
            search: None,
            page: Some(1),
        })
        .await?;
        format!(
            r#"<div id="schedules-content" hx-swap-oob="outerHTML">{}</div>"#,
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

// Routes export
pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(schedules_list).post(schedules_create))
        .at("/search", get(schedules_search))
        .at("/new", get(schedules_new))
        .at(
            "/:id",
            get(schedules_edit)
                .post(schedules_update)
                .delete(schedules_delete),
        )
}
