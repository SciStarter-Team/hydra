// Entity (slug) management for admin

use crate::component::{AdminLayout, Component};
use crate::entity::{Entity, EntityKind, EntityTarget};
use crate::middleware::authorization::Authenticated;
use crate::page_change::entity_change;
use crate::pagecontent::PageContentKind;
use crate::peering::LoadStorePeered;
use common::message::log::{Housekeeping, PageChange};
use common::shared::db;
use poem::{
    Endpoint, Route, get, handler, post,
    web::{Form, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

// Templates

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/entities.stpl.html")]
pub struct EntitiesPage {
    pub entities_content: EntitiesContent,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/entities_content.stpl.html")]
pub struct EntitiesContent {
    pub entities: Vec<EntityWithInfo>,
    pub current_page: i64,
    pub total_entities: i64,
    pub has_previous: bool,
    pub has_next: bool,
    pub per_page: i64,
    pub start_item: i64,
    pub end_item: i64,
    pub pagination_url: String,
    pub pagination_include: String,
    pub search_term: String,
    pub selected_kind: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/entity_row.stpl.html")]
pub struct EntityRow {
    pub entity: EntityWithInfo,
    pub oob: bool,
    pub oob_swap: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/modals/new_entity.stpl.html")]
pub struct NewEntityForm {
    pub title: String,
    pub error_message: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/pages/edit_entity.stpl.html")]
pub struct EditEntityPage {
    pub slug: String,
    pub kind: String,
    pub target: String,
    pub target_ulid: Option<Ulid>,
    pub target_input_template: EntityTargetInputTemplate,
    pub password: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/entity_target_input.stpl.html")]
pub struct EntityTargetInputTemplate {
    pub kind: EntityKind,
    pub current_value: String,
    pub project_widget: Option<crate::admin::autocomplete::EntityAutocomplete>,
    pub event_widget: Option<crate::admin::autocomplete::EntityAutocomplete>,
    pub pagecontent_widget: Option<crate::admin::autocomplete::EntityAutocomplete>,
    pub pagebuilder_widget: Option<crate::admin::autocomplete::EntityAutocomplete>,
}

// Types

#[derive(Debug, Clone)]
pub struct EntityWithInfo {
    pub slug: String,
    pub kind: EntityKind,
    pub target: String,
    pub target_info: Option<String>,
    pub published: bool,
    pub password: Option<String>,
}

#[derive(Debug)]
struct EntityDbRow {
    slug: String,
    kind: EntityKind,
    target: String,
    publish: bool,
    created: common::timestring::TimeString,
    password: Option<String>,
}

// Query and form types

#[derive(Debug, Deserialize)]
pub struct CreateEntityForm {
    pub slug: String,
    pub kind: String,
    pub target: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateEntityForm {
    pub slug: String,
    pub kind: String,
    pub target: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EntitySearchQuery {
    pub search: Option<String>,
    pub kind: Option<String>,
    pub page: Option<i32>,
    pub per_page: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct EntityTargetInputQuery {
    pub kind: String,
    pub current_value: Option<String>,
    pub mode: Option<String>,
}

#[derive(Deserialize)]
struct EntityPublishForm {
    publish: Option<String>,
}

// Entity Kind Helper Functions
impl EntityKind {
    pub fn display_name(&self) -> &'static str {
        match self {
            EntityKind::WordPress => "WordPress Page",
            EntityKind::Redirect => "Redirect",
            EntityKind::RawHtml => "Raw HTML",
            EntityKind::DressedHtml => "Dressed HTML",
            EntityKind::Wysiwyg => "Simple Editor",
            EntityKind::PageBuilder => "Component Page",
            EntityKind::Form => "Form",
            EntityKind::Project => "Project",
            EntityKind::Event => "Event",
            EntityKind::Placeholder => "Placeholder",
        }
    }

    pub fn all_kinds() -> Vec<EntityKind> {
        vec![
            EntityKind::Project,
            EntityKind::Event,
            EntityKind::WordPress,
            EntityKind::Redirect,
            EntityKind::RawHtml,
            EntityKind::Wysiwyg,
            EntityKind::PageBuilder,
            EntityKind::Form,
            EntityKind::Placeholder,
        ]
    }
}

// Handlers

#[handler]
async fn entities(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let entities_content = fetch_entities_content(&EntitySearchQuery {
        search: None,
        kind: None,
        page: Some(1),
        per_page: Some(25),
    })
    .await?;

    let page = EntitiesPage { entities_content };
    let prepared = AdminLayout::new("Slug Management", page)
        .with_breadcrumb("Slugs", None)
        .with_sidebar_active("entities")
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn entities_search(
    Query(query): Query<EntitySearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let entities_content = fetch_entities_content(&query).await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(entities_content.render_once()?))
}

#[handler]
async fn entities_target_input(
    Query(query): Query<EntityTargetInputQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = query.kind.parse::<EntityKind>()?;
    let current_value = query.current_value.unwrap_or_default();

    let template = create_target_input_template(&kind, &current_value).await;
    let html = template.render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn entities_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let form = NewEntityForm {
        title: "Add New Entity".to_string(),
        error_message: None,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}

#[handler]
async fn entities_create(
    Form(form): Form<CreateEntityForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    match (async || -> Result<(String, Option<Ulid>, EntityKind), anyhow::Error> {
        if form.slug.trim().is_empty() {
            return Err(anyhow::anyhow!("Slug is required"));
        }

        if Entity::slug_exists(&form.slug).await? {
            return Err(anyhow::anyhow!("Slug already exists"));
        }

        let kind = form.kind.parse::<EntityKind>()?;
        let target = parse_target_for_kind(&kind, form.target.as_deref())?;

        let entity = Entity::new(&form.slug, kind, target);
        entity.store().await?;

        Ok((form.slug.clone(), entity.target().as_ulid(), kind))
    })()
    .await
    {
        Ok((slug, target, kind)) => {
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    entity_change(&kind, PageChange::EntityCreate),
                    user_info.user_id,
                    target,
                    format!("slug={} kind={}", slug, form.kind),
                )
                .await?;
            }

            let entities_content = fetch_entities_content(&EntitySearchQuery {
                search: None,
                kind: None,
                page: Some(1),
                per_page: Some(25),
            })
            .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(format!(
                    r#"<div hx-swap-oob="innerHTML:#entities-content">{}</div>"#,
                    entities_content.render_once()?
                )))
        }
        Err(e) => {
            let form = NewEntityForm {
                title: "Add New Entity".to_string(),
                error_message: Some(e.to_string()),
            };

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(form.render_once()?))
        }
    }
}

#[handler]
async fn entities_edit(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let entity = Entity::load(slug.clone()).await?;

    let target_ulid = match entity.target() {
        EntityTarget::Ulid(ulid) => Some(*ulid),
        _ => None,
    };

    let target_input_template =
        create_target_input_template(entity.kind(), &entity.target().to_string()).await;

    let content = EditEntityPage {
        slug: slug.clone(),
        kind: entity.kind().as_ref().to_string(),
        target: entity.target().to_string(),
        target_ulid,
        target_input_template,
        password: entity.password().map(|s| s.to_string()),
        error_message: None,
    };

    let prepared = AdminLayout::new("Edit Slug", content)
        .with_sidebar_active("entities")
        .with_breadcrumb("Slugs", Some("/-admin/entities"))
        .with_breadcrumb("Edit", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn entities_update(
    Path(slug): Path<String>,
    Form(form): Form<UpdateEntityForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let kind = match form.kind.parse::<EntityKind>() {
        Ok(k) => k,
        Err(e) => {
            let entity = Entity::load(slug.clone()).await?;
            let target_ulid = match entity.target() {
                EntityTarget::Ulid(ulid) => Some(*ulid),
                _ => None,
            };
            let target_input_template =
                create_target_input_template(entity.kind(), &entity.target().to_string()).await;

            let content = EditEntityPage {
                slug: slug.clone(),
                kind: form.kind,
                target: form.target.unwrap_or_default(),
                target_ulid,
                target_input_template,
                password: form.password,
                error_message: Some(format!("Invalid entity kind: {}", e)),
            };

            let prepared = AdminLayout::new("Edit Slug", content)
                .with_sidebar_active("entities")
                .with_breadcrumb("Slugs", Some("/-admin/entities"))
                .with_breadcrumb("Edit", None)
                .prepare(user_info)
                .await?;

            return Ok(poem::Response::builder()
                .content_type("text/html")
                .body(prepared.render_once()?));
        }
    };

    let target = match parse_target_for_kind(&kind, form.target.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            let entity = Entity::load(slug.clone()).await?;
            let target_ulid = match entity.target() {
                EntityTarget::Ulid(ulid) => Some(*ulid),
                _ => None,
            };
            let target_input_template =
                create_target_input_template(&kind, form.target.as_deref().unwrap_or("")).await;

            let content = EditEntityPage {
                slug: slug.clone(),
                kind: form.kind.clone(),
                target: form.target.unwrap_or_default(),
                target_ulid,
                target_input_template,
                password: form.password,
                error_message: Some(format!("Invalid target: {}", e)),
            };

            let prepared = AdminLayout::new("Edit Slug", content)
                .with_sidebar_active("entities")
                .with_breadcrumb("Slugs", Some("/-admin/entities"))
                .with_breadcrumb("Edit", None)
                .prepare(user_info)
                .await?;

            return Ok(poem::Response::builder()
                .content_type("text/html")
                .body(prepared.render_once()?));
        }
    };

    // Load existing entity and update fields (convert empty password string to None)
    let password = form.password.filter(|p| !p.trim().is_empty());
    let updated_entity = Entity::load(slug.clone())
        .await?
        .with_kind(kind)
        .with_target(target)
        .with_password(password);
    match updated_entity.store().await {
        Ok(_) => {
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    entity_change(updated_entity.kind(), PageChange::EntityUpdate),
                    user_info.user_id,
                    updated_entity.target().as_ulid(),
                    format!("slug={} kind={}", slug, form.kind),
                )
                .await?;
            }

            Ok(poem::Response::builder()
                .status(poem::http::StatusCode::SEE_OTHER)
                .header("Location", "/-admin/entities")
                .finish())
        }
        Err(e) => {
            let entity = Entity::load(slug.clone()).await?;
            let target_ulid = match entity.target() {
                EntityTarget::Ulid(ulid) => Some(*ulid),
                _ => None,
            };
            let target_input_template =
                create_target_input_template(entity.kind(), &entity.target().to_string()).await;

            let content = EditEntityPage {
                slug: slug.clone(),
                kind: entity.kind().as_ref().to_string(),
                target: entity.target().to_string(),
                target_ulid,
                target_input_template,
                password: entity.password().map(|s| s.to_string()),
                error_message: Some(format!("Error saving entity: {}", e)),
            };

            let prepared = AdminLayout::new("Edit Slug", content)
                .with_sidebar_active("entities")
                .with_breadcrumb("Slugs", Some("/-admin/entities"))
                .with_breadcrumb("Edit", None)
                .prepare(user_info)
                .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(prepared.render_once()?))
        }
    }
}

#[handler]
async fn entities_delete(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let entity = Entity::load(slug.clone()).await.ok();

    sqlx::query!(r#"DELETE FROM "entities" WHERE "slug" = ?"#, slug)
        .execute(db())
        .await?;

    if let Some(user_info) = &user_info {
        let extra = entity
            .as_ref()
            .map(|e| format!("slug={} kind={:?}", slug, e.kind()));
        common::service::log_activity(None, "entity-delete", user_info.user_id, None, extra)
            .await?;
    }

    let exists: bool =
        sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM entities) as "exists: bool""#)
            .fetch_one(db())
            .await?;

    let html = if !exists {
        let content = fetch_entities_content(&EntitySearchQuery {
            search: None,
            kind: None,
            page: Some(1),
            per_page: None,
        })
        .await?;
        format!(
            r#"<div id="entities-content" hx-swap-oob="outerHTML">{}</div>"#,
            content.render_once()?
        )
    } else {
        String::new()
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

/// Tell an event's submitter that it is published, with what the mail names:
/// the event and a link to it.
async fn announce_event_published(target: &EntityTarget) -> Result<(), anyhow::Error> {
    let EntityTarget::Ulid(event_ulid) = target else {
        return Ok(());
    };

    let event_id = event_ulid.to_string();
    let event = sqlx::query!(
        r#"SELECT e.name, e.page_creator_id,
                  (SELECT n.slug FROM entities n
                    WHERE n.kind = 'Event' AND n.target = e.event_id) as "slug?"
             FROM events e WHERE e.event_id = ?"#,
        event_id
    )
    .fetch_one(db())
    .await?;

    let creator: Ulid = event.page_creator_id.parse().map_err(|_| {
        anyhow::anyhow!(
            "page_creator_id {:?} is not a user id",
            event.page_creator_id
        )
    })?;

    common::service::trigger_email(
        common::service::TriggerEvent::EventPublished,
        creator,
        Some(&[
            ("project_name".to_string(), event.name),
            ("project_id".to_string(), event_id),
            (
                "project_link".to_string(),
                format!(
                    "https://{}/events/{}",
                    env!("DOMAIN"),
                    event.slug.unwrap_or_default()
                ),
            ),
        ]),
    )
    .await
}

#[handler]
async fn entities_publish_toggle(
    Path(slug): Path<String>,
    Form(form): Form<EntityPublishForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, poem::Error> {
    // Parse the publish value - hx-vals sends "true" or "false" based on switch checked state
    let publish = form.publish.as_deref() == Some("true");

    // Load the entity
    let mut entity = Entity::load(slug.clone())
        .await
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::NOT_FOUND))?;

    let updated_entity = entity.with_publish(publish);

    updated_entity
        .store()
        .await
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;

    if let Some(user_info) = &user_info {
        let _ = common::service::log_activity(
            None,
            if publish {
                Housekeeping::EntityPublish
            } else {
                Housekeeping::EntityUnpublish
            },
            user_info.user_id,
            updated_entity.target().as_ulid(),
            Some(slug.clone()),
        )
        .await;
    }

    // Publishing an event here is what publishing a project is in
    // `projects_publish`: the moment its submitter is told. Events reach this
    // generic toggle rather than a kind-specific handler, so the notice is
    // raised from here. Non-fatal -- the entity is published either way.
    if publish && matches!(updated_entity.kind(), EntityKind::Event) {
        if let Err(error) = announce_event_published(updated_entity.target()).await {
            eprintln!("Publish notice for event {slug} not sent: {error}");
        }
    }

    let entity_with_info = create_entity_with_info(&updated_entity)
        .await
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;

    let entity_row = EntityRow {
        entity: entity_with_info,
        oob: true,
        oob_swap: "true".to_string(),
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(entity_row.render_once().unwrap()))
}

// Helper Functions

async fn create_pagecontent_widget(
    current_value: &str,
    pagecontent_kind: PageContentKind,
) -> crate::admin::autocomplete::EntityAutocomplete {
    if !current_value.is_empty() {
        if let Ok(ulid) = current_value.parse::<Ulid>() {
            if let Ok(Some(name)) = crate::admin::autocomplete::resolve_pagecontent_name(ulid).await
            {
                return crate::admin::autocomplete::EntityAutocomplete::new_pagecontent_selector(
                    "target",
                    "Target",
                    "Search for content...",
                )
                .with_selected(ulid, name)
                .with_kind_filter(pagecontent_kind);
            }
        }
    }
    crate::admin::autocomplete::EntityAutocomplete::new_pagecontent_selector(
        "target",
        "Target",
        "Search for content...",
    )
    .with_kind_filter(pagecontent_kind)
}

async fn create_target_input_template(
    kind: &EntityKind,
    current_value: &str,
) -> EntityTargetInputTemplate {
    let (project_widget, event_widget, pagecontent_widget, pagebuilder_widget) = match kind {
        EntityKind::Project => {
            let widget = if !current_value.is_empty() {
                if let Ok(ulid) = current_value.parse::<Ulid>() {
                    if let Ok(Some(name)) =
                        crate::admin::autocomplete::resolve_project_name(ulid).await
                    {
                        crate::admin::autocomplete::EntityAutocomplete::new_project_selector(
                            "target",
                            "Target",
                            "Search for a project...",
                        )
                        .with_selected(ulid, name)
                    } else {
                        crate::admin::autocomplete::EntityAutocomplete::new_project_selector(
                            "target",
                            "Target",
                            "Search for a project...",
                        )
                    }
                } else {
                    crate::admin::autocomplete::EntityAutocomplete::new_project_selector(
                        "target",
                        "Target",
                        "Search for a project...",
                    )
                }
            } else {
                crate::admin::autocomplete::EntityAutocomplete::new_project_selector(
                    "target",
                    "Target",
                    "Search for a project...",
                )
            };
            (Some(widget), None, None, None)
        }
        EntityKind::Event => {
            let widget = if !current_value.is_empty() {
                if let Ok(ulid) = current_value.parse::<Ulid>() {
                    if let Ok(Some(name)) =
                        crate::admin::autocomplete::resolve_event_name(ulid).await
                    {
                        crate::admin::autocomplete::EntityAutocomplete::new_event_selector(
                            "target",
                            "Target",
                            "Search for an event...",
                        )
                        .with_selected(ulid, name)
                    } else {
                        crate::admin::autocomplete::EntityAutocomplete::new_event_selector(
                            "target",
                            "Target",
                            "Search for an event...",
                        )
                    }
                } else {
                    crate::admin::autocomplete::EntityAutocomplete::new_event_selector(
                        "target",
                        "Target",
                        "Search for an event...",
                    )
                }
            } else {
                crate::admin::autocomplete::EntityAutocomplete::new_event_selector(
                    "target",
                    "Target",
                    "Search for an event...",
                )
            };
            (None, Some(widget), None, None)
        }
        EntityKind::RawHtml => {
            let widget = create_pagecontent_widget(current_value, PageContentKind::RawHtml).await;
            (None, None, Some(widget), None)
        }
        EntityKind::DressedHtml => {
            let widget =
                create_pagecontent_widget(current_value, PageContentKind::DressedHtml).await;
            (None, None, Some(widget), None)
        }
        EntityKind::Wysiwyg => {
            let widget = create_pagecontent_widget(current_value, PageContentKind::Wysiwyg).await;
            (None, None, Some(widget), None)
        }
        EntityKind::PageBuilder => {
            let widget = if !current_value.is_empty() {
                if let Ok(ulid) = current_value.parse::<Ulid>() {
                    if let Ok(Some(name)) =
                        crate::admin::autocomplete::resolve_pagebuilder_page_name(ulid).await
                    {
                        crate::admin::autocomplete::EntityAutocomplete::new_pagebuilder_selector(
                            "target",
                            "Target",
                            "Search for a component page...",
                        )
                        .with_selected(ulid, name)
                    } else {
                        crate::admin::autocomplete::EntityAutocomplete::new_pagebuilder_selector(
                            "target",
                            "Target",
                            "Search for a component page...",
                        )
                    }
                } else {
                    crate::admin::autocomplete::EntityAutocomplete::new_pagebuilder_selector(
                        "target",
                        "Target",
                        "Search for a component page...",
                    )
                }
            } else {
                crate::admin::autocomplete::EntityAutocomplete::new_pagebuilder_selector(
                    "target",
                    "Target",
                    "Search for a component page...",
                )
            };
            (None, None, None, Some(widget))
        }
        EntityKind::Form => {
            let widget = create_pagecontent_widget(current_value, PageContentKind::Form).await;
            (None, None, Some(widget), None)
        }
        _ => (None, None, None, None),
    };

    EntityTargetInputTemplate {
        kind: *kind,
        current_value: current_value.to_string(),
        project_widget,
        event_widget,
        pagecontent_widget,
        pagebuilder_widget,
    }
}

fn parse_target_for_kind(
    kind: &EntityKind,
    target_str: Option<&str>,
) -> Result<EntityTarget, anyhow::Error> {
    EntityTarget::parse_for_kind(*kind, target_str)
}

async fn fetch_entities_content(
    query: &EntitySearchQuery,
) -> Result<EntitiesContent, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1) as i64;
    let per_page = query.per_page.unwrap_or(25).min(100).max(1) as i64;
    let offset = (page - 1) * per_page;

    let search_term = query.search.as_deref().unwrap_or("").trim();
    let selected_kind = query.kind.as_deref().unwrap_or("");

    let all_rows = sqlx::query_as!(
        EntityDbRow,
        r#"SELECT "slug", "kind" as "kind: EntityKind", "target", "publish", "created" as "created: common::timestring::TimeString", "password" FROM "entities" ORDER BY "created" DESC"#
    ).fetch_all(db()).await?;

    let filtered_rows: Vec<_> = all_rows
        .into_iter()
        .filter(|row| {
            if !search_term.is_empty()
                && !row
                    .slug
                    .to_lowercase()
                    .contains(&search_term.to_lowercase())
            {
                return false;
            }
            if !selected_kind.is_empty() && row.kind.as_ref() != selected_kind {
                return false;
            }
            true
        })
        .collect();

    let total_entities = filtered_rows.len() as i64;

    let entity_rows: Vec<_> = filtered_rows
        .into_iter()
        .skip(offset as usize)
        .take(per_page as usize)
        .collect();

    let mut entity_list = Vec::new();
    for row in entity_rows {
        let target = parse_database_target(row.kind, &row.target)?;
        let entity =
            Entity::new_with_created(&row.slug, row.kind, target, row.publish, row.created)
                .with_password(row.password);
        let entity_with_info = create_entity_with_info(&entity).await?;
        entity_list.push(entity_with_info);
    }

    let has_previous = page > 1;
    let has_next = (page * per_page) < total_entities;
    let start_item = if total_entities > 0 { offset + 1 } else { 0 };
    let end_item = std::cmp::min(page * per_page, total_entities);

    Ok(EntitiesContent {
        entities: entity_list,
        current_page: page,
        total_entities,
        has_previous,
        has_next,
        per_page,
        start_item,
        end_item,
        pagination_url: "/-admin/entities/search".to_string(),
        pagination_include: "#entity-search-form".to_string(),
        search_term: search_term.to_string(),
        selected_kind: selected_kind.to_string(),
    })
}

fn parse_database_target(
    kind: EntityKind,
    target_str: &str,
) -> Result<EntityTarget, anyhow::Error> {
    if target_str.trim().is_empty() {
        return Ok(EntityTarget::Nil);
    }

    match kind {
        EntityKind::Redirect => match target_str.parse() {
            Ok(url) => Ok(EntityTarget::Url(url)),
            Err(_) => Ok(EntityTarget::Nil),
        },
        EntityKind::WordPress => Ok(EntityTarget::String(target_str.to_string())),
        EntityKind::RawHtml
        | EntityKind::DressedHtml
        | EntityKind::Wysiwyg
        | EntityKind::PageBuilder
        | EntityKind::Form
        | EntityKind::Project
        | EntityKind::Event => match target_str.parse() {
            Ok(ulid) => Ok(EntityTarget::Ulid(ulid)),
            Err(_) => Ok(EntityTarget::Nil),
        },
        EntityKind::Placeholder => Ok(EntityTarget::Nil),
    }
}

async fn create_entity_with_info(entity: &Entity) -> Result<EntityWithInfo, anyhow::Error> {
    let target_info = match (entity.kind(), entity.target()) {
        (EntityKind::Project, EntityTarget::Ulid(ulid)) => {
            let ulid_str = ulid.to_string();
            let result = sqlx::query!(
                r#"SELECT "name" FROM "projects" WHERE "project_id" = ?"#,
                ulid_str
            )
            .fetch_optional(db())
            .await?;

            result
                .map(|row| row.name)
                .or(Some("Project not found".to_string()))
        }
        (EntityKind::Event, EntityTarget::Ulid(ulid)) => {
            let ulid_str = ulid.to_string();
            let result = sqlx::query!(
                r#"SELECT "name" FROM "events" WHERE "event_id" = ?"#,
                ulid_str
            )
            .fetch_optional(db())
            .await?;

            result
                .map(|row| row.name)
                .or(Some("Event not found".to_string()))
        }
        (EntityKind::Redirect, EntityTarget::Url(url)) => Some(url.to_string()),
        (EntityKind::WordPress, EntityTarget::String(s)) => Some(format!("WordPress: {}", s)),
        (
            EntityKind::RawHtml
            | EntityKind::DressedHtml
            | EntityKind::Wysiwyg
            | EntityKind::PageBuilder
            | EntityKind::Form,
            _,
        ) => Some("Static content".to_string()),
        (_, EntityTarget::Nil) => None,
        _ => Some("Invalid target type".to_string()),
    };

    Ok(EntityWithInfo {
        slug: entity.slug().to_string(),
        kind: *entity.kind(),
        target: entity.target().to_string(),
        target_info,
        published: entity.publish(),
        password: entity.password().map(|s| s.to_string()),
    })
}

// Routes export

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(entities))
        .at("/search", get(entities_search))
        .at("/new", get(entities_new).post(entities_create))
        .at("/target-input", get(entities_target_input))
        .at(
            "/edit/*slug",
            get(entities_edit)
                .post(entities_update)
                .delete(entities_delete),
        )
        .at("/publish/*slug", post(entities_publish_toggle))
}
