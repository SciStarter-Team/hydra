//! Exchange management for SciStarter staff.
//!
//! Every setting an exchange has is edited here, the way orrery's Django
//! admin edits them. An exchange's own people have the narrower settings
//! page at `/exchange/<path_id>/org.html`, which also holds its staff list,
//! prompted searches and approval queue.

use std::collections::BTreeSet;

use anyhow::Error;
use common::message::user::{
    UserByEmailRequest, UserByEmailResponse, UserDetailsBatchGetRequest,
    UserDetailsBatchGetResponse,
};
use common::service::request;
use common::shared::db;
use poem::{
    Endpoint, IntoResponse, Route, get, handler,
    web::{Form, Html, Path},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::component::{AdminLayout, Component};
use crate::exchange::handlers::search_error;
use crate::exchange::{Exchange, ExchangeSettings, access};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;

struct ExchangeListItem {
    exchange_id: Ulid,
    path_id: i64,
    slug: String,
    name: String,
    description: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/exchanges.stpl.html")]
struct ExchangesListPage {
    exchanges: Vec<ExchangeListItem>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/exchanges_create.stpl.html")]
struct ExchangeCreatePage {
    form: ExchangeCreateForm,
    error: Option<String>,
}

/// A project an exchange has hidden, as the edit page lists it.
struct HiddenProjectRow {
    slug: Option<String>,
    name: String,
    note: String,
    elevate: bool,
    hidden_at: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/exchanges_edit.stpl.html")]
struct ExchangeEditPage {
    exchange: Exchange,
    regions: Vec<(i64, String)>,
    owner_email: String,
    hidden: Vec<HiddenProjectRow>,
    error: Option<String>,
}

impl ExchangeEditPage {
    fn lines(values: &[String]) -> String {
        values.join("\n")
    }
}

#[derive(Debug, Default, Deserialize)]
struct ExchangeCreateForm {
    #[serde(default)]
    path_id: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    logo_url: String,
}

#[derive(Debug, Deserialize)]
struct ExchangeUpdateForm {
    slug: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    logo_url: String,
    #[serde(default)]
    owner_email: String,
    /// Empty means "no region": a view of the whole catalogue.
    #[serde(default)]
    region_id: String,
    #[serde(default)]
    restricted_add: bool,
    #[serde(default)]
    global_by_default: bool,
    #[serde(default)]
    exclude_scistarter: bool,
    #[serde(default)]
    search: String,
    #[serde(default)]
    include_tags: String,
    #[serde(default)]
    include_origins: String,
    #[serde(default)]
    excluded_origins: String,
}

/// One entry per non-blank line, each trimmed, each once.
fn lines_of(text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| seen.insert(line.to_string()))
        .map(str::to_string)
        .collect()
}

fn optional(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Why another exchange's `path_id` or `slug` would collide with these.
async fn collision(exchange_id: Ulid, path_id: i64, slug: &str) -> Result<Option<String>, Error> {
    let exchange_id = exchange_id.to_string();
    let other = sqlx::query!(
        r#"SELECT name, path_id, slug FROM exchanges
           WHERE exchange_id != ? AND (path_id = ? OR slug = ?)"#,
        exchange_id,
        path_id,
        slug
    )
    .fetch_optional(db())
    .await?;

    Ok(other.map(|other| {
        if other.path_id == path_id {
            format!("Path ID {path_id} already belongs to {}.", other.name)
        } else {
            format!("Slug \"{slug}\" already belongs to {}.", other.name)
        }
    }))
}

async fn render(
    user_info: Option<std::sync::Arc<common::message::user::BasicInfo>>,
    title: &str,
    content: impl TemplateOnce,
) -> Result<poem::Response, Error> {
    let prepared = AdminLayout::new(title, content)
        .with_sidebar_active("engine-room-exchanges")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn exchanges_list(Authenticated(user_info): Authenticated) -> Result<poem::Response, Error> {
    let exchanges = sqlx::query!(
        "SELECT exchange_id, path_id, slug, name, description FROM exchanges ORDER BY name"
    )
    .fetch_all(db())
    .await?
    .into_iter()
    .filter_map(|row| {
        Ulid::from_string(&row.exchange_id)
            .ok()
            .map(|exchange_id| ExchangeListItem {
                exchange_id,
                path_id: row.path_id,
                slug: row.slug,
                name: row.name,
                description: row.description,
            })
    })
    .collect();

    render(
        user_info,
        "Exchange Management",
        ExchangesListPage { exchanges },
    )
    .await
}

#[handler]
async fn exchange_create_page(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    render(
        user_info,
        "Create Exchange",
        ExchangeCreatePage {
            form: ExchangeCreateForm::default(),
            error: None,
        },
    )
    .await
}

#[handler]
async fn exchange_create(
    Authenticated(user_info): Authenticated,
    Form(form): Form<ExchangeCreateForm>,
) -> Result<poem::Response, Error> {
    let staff = user_info
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let exchange_id = Ulid::new();
    let slug = form.slug.trim().to_string();

    let error = match form.path_id.trim().parse::<i64>() {
        Err(_) => Some("The path ID must be a whole number.".to_string()),
        Ok(path_id) => collision(exchange_id, path_id, &slug).await?,
    };

    if let Some(error) = error {
        return render(
            user_info,
            "Create Exchange",
            ExchangeCreatePage {
                form,
                error: Some(error),
            },
        )
        .await;
    }

    let exchange = Exchange {
        exchange_id,
        path_id: form.path_id.trim().parse()?,
        slug,
        name: form.name.trim().to_string(),
        description: form.description.trim().to_string(),
        logo_url: optional(&form.logo_url),
        settings: ExchangeSettings {
            allow_project_voting: true,
            allow_project_reviews: true,
            // A new exchange is a view of the whole catalogue until somebody
            // gives it a region to be a view of.
            ..Default::default()
        },
        predefined_searches: Vec::new(),
        hidden_projects: Default::default(),
    };

    exchange.store().await?;

    common::service::log_activity(
        None,
        "exchange-create",
        staff.user_id,
        Some(exchange.exchange_id),
        None,
    )
    .await?;

    Ok(
        poem::web::Redirect::see_other(format!("/-admin/exchanges/{}", exchange.exchange_id))
            .into_response(),
    )
}

/// The edit page for `exchange`, showing `error` if there is one.
async fn edit_page(
    user_info: Option<std::sync::Arc<common::message::user::BasicInfo>>,
    exchange: Exchange,
    owner_email: Option<String>,
    error: Option<String>,
) -> Result<poem::Response, Error> {
    let owner_email = match owner_email {
        Some(email) => email,
        None => {
            let owner = access::people(exchange.exchange_id)
                .await?
                .into_iter()
                .find(|(_, role)| *role == access::ExchangeRole::Owner);

            match owner {
                Some((user_id, _)) => request::<UserDetailsBatchGetResponse>(
                    "user.admin.user_batch_get",
                    UserDetailsBatchGetRequest {
                        user_ids: vec![user_id],
                    },
                )
                .await?
                .users
                .into_iter()
                .next()
                .map(|user| user.primary_email)
                .unwrap_or_default(),
                None => String::new(),
            }
        }
    };

    let exchange_id = exchange.exchange_id.to_string();
    let hidden = sqlx::query!(
        r#"SELECT h.note, h.elevate as "elevate: bool", h.hidden_at,
                  p.name as "name?", ent.slug as "slug?"
           FROM exchange_hidden_projects h
           LEFT JOIN projects p ON p.project_id = h.project_id
           LEFT JOIN entities ent ON ent.target = h.project_id AND ent.kind = 'Project'
           WHERE h.exchange_id = ?
           ORDER BY h.elevate DESC, p.name"#,
        exchange_id
    )
    .fetch_all(db())
    .await?
    .into_iter()
    .map(|row| HiddenProjectRow {
        slug: row.slug,
        name: row.name.unwrap_or_default(),
        note: row.note,
        elevate: row.elevate,
        hidden_at: row.hidden_at,
    })
    .collect();

    render(
        user_info,
        "Edit Exchange",
        ExchangeEditPage {
            exchange,
            regions: crate::region::all().await?,
            owner_email,
            hidden,
            error,
        },
    )
    .await
}

#[handler]
async fn exchange_edit_page(
    Path(exchange_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let exchange = Exchange::load(Ulid::from_string(&exchange_id)?).await?;
    edit_page(user_info, exchange, None, None).await
}

#[handler]
async fn exchange_update(
    Path(exchange_id): Path<String>,
    Authenticated(user_info): Authenticated,
    Form(form): Form<ExchangeUpdateForm>,
) -> Result<poem::Response, Error> {
    let staff = user_info
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let mut exchange = Exchange::load(Ulid::from_string(&exchange_id)?).await?;

    exchange.slug = form.slug.trim().to_string();
    exchange.name = form.name.trim().to_string();
    exchange.description = form.description.trim().to_string();
    exchange.logo_url = optional(&form.logo_url);
    exchange.settings.region_id = form.region_id.trim().parse().ok();
    exchange.settings.restricted_add = form.restricted_add;
    exchange.settings.global_by_default = form.global_by_default;
    exchange.settings.exclude_scistarter = form.exclude_scistarter;
    exchange.settings.search = form.search.trim().to_string();
    exchange.settings.include_tags = lines_of(&form.include_tags);
    exchange.settings.include_origins = lines_of(&form.include_origins);
    exchange.settings.excluded_origins = lines_of(&form.excluded_origins);

    let owner_email = form.owner_email.trim().to_string();
    let owner = match optional(&owner_email) {
        None => None,
        Some(email) => {
            let found = request::<UserByEmailResponse>(
                "user.search.by_email",
                UserByEmailRequest {
                    email: email.clone(),
                },
            )
            .await?;

            match found.user_id.filter(|_| found.found) {
                Some(user_id) => Some(Ulid::from_string(&user_id)?),
                None => {
                    let error = format!("No account has the email address {email}.");
                    return edit_page(user_info, exchange, Some(owner_email), Some(error)).await;
                }
            }
        }
    };

    let error = match search_error(&exchange.settings.search) {
        Some(error) => Some(format!(
            "The search is not a usable regular expression: {error}"
        )),
        None => collision(exchange.exchange_id, exchange.path_id, &exchange.slug).await?,
    };

    if let Some(error) = error {
        return edit_page(user_info, exchange, Some(owner_email), Some(error)).await;
    }

    exchange.store().await?;

    let people = access::people(exchange.exchange_id).await?;
    let current = people
        .iter()
        .find(|(_, role)| *role == access::ExchangeRole::Owner)
        .map(|(user_id, _)| *user_id);

    if current != owner {
        if let Some(current) = current {
            access::revoke(exchange.exchange_id, current).await?;
        }
        if let Some(owner) = owner {
            access::grant(
                exchange.exchange_id,
                owner,
                access::ExchangeRole::Owner,
                &staff.user_id.to_string(),
            )
            .await?;
        }
    }

    common::service::log_activity(
        None,
        "exchange-update",
        staff.user_id,
        Some(exchange.exchange_id),
        None,
    )
    .await?;

    Ok(poem::web::Redirect::see_other("/-admin/exchanges").into_response())
}

#[handler]
async fn exchange_delete(
    Path(exchange_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, Error> {
    let staff = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let exchange_id = Ulid::from_string(&exchange_id)?;

    Exchange::delete(exchange_id).await?;

    common::service::log_activity(
        None,
        "exchange-delete",
        staff.user_id,
        Some(exchange_id),
        None,
    )
    .await?;

    Ok(Html("<div class=\"text-green-600\">Exchange deleted successfully</div>").into_response())
}

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(exchanges_list))
        .at("/create", get(exchange_create_page).post(exchange_create))
        .at(
            "/:id",
            get(exchange_edit_page)
                .post(exchange_update)
                .delete(exchange_delete),
        )
}
