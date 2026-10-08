// OAuth2 client management for admin

use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use crate::oauth2::types::Client;
use crate::peering::LoadStorePeered;
use common::shared::db;
use poem::{
    Endpoint, IntoResponse, Route, get, handler,
    web::{Form, Html, Path},
};
use sailfish::TemplateOnce;
use serde::Deserialize;

// ==================== OAuth2 Templates ====================

#[derive(Clone)]
struct OAuth2ClientListItem {
    client_id: String,
    client_name: String,
    owner_user_id: Option<String>,
    redirect_uris: Vec<String>,
    allowed_scopes: Vec<String>,
    created: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/oauth2_clients.stpl.html")]
struct OAuth2ClientsListPage {
    clients: Vec<OAuth2ClientListItem>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/oauth2_clients_create.stpl.html")]
struct OAuth2ClientCreatePage {}

#[derive(TemplateOnce)]
#[template(path = "admin/oauth2_clients_edit.stpl.html")]
struct OAuth2ClientEditPage {
    client_id: String,
    client_name: String,
    client_secret: String,
    owner_user_id: String,
    redirect_uris: String,
    allowed_scopes: String,
}

// ==================== OAuth2 Forms ====================

#[derive(Debug, Deserialize)]
struct OAuth2ClientCreateForm {
    client_id: String,
    client_name: String,
    client_secret: String,
    owner_user_id: String,
    redirect_uris: String,
    allowed_scopes: String,
}

#[derive(Debug, Deserialize)]
struct OAuth2ClientUpdateForm {
    client_name: String,
    client_secret: String,
    owner_user_id: String,
    redirect_uris: String,
    allowed_scopes: String,
}

// ==================== OAuth2 Validation Helpers ====================

fn validate_client_id(client_id: &str) -> Result<(), anyhow::Error> {
    // Only allow alphanumeric, hyphens, underscores
    if !client_id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!(
            "Client ID must contain only alphanumeric characters, hyphens, and underscores"
        );
    }
    if client_id.is_empty() || client_id.len() > 64 {
        anyhow::bail!("Client ID must be 1-64 characters");
    }
    Ok(())
}

fn validate_redirect_uri(uri: &str) -> Result<(), anyhow::Error> {
    let parsed =
        url::Url::parse(uri).map_err(|e| anyhow::anyhow!("Invalid URI '{}': {}", uri, e))?;

    // Require HTTPS in production (allow http://localhost for dev)
    if parsed.scheme() != "https" {
        if cfg!(debug_assertions) {
            // Allow http://localhost in development
            if parsed.scheme() != "http"
                || !parsed.host_str().unwrap_or("").starts_with("localhost")
            {
                anyhow::bail!(
                    "Only HTTPS URIs allowed (except http://localhost in dev): {}",
                    uri
                );
            }
        } else {
            anyhow::bail!("Only HTTPS redirect URIs allowed in production: {}", uri);
        }
    }

    // No fragment allowed
    if parsed.fragment().is_some() {
        anyhow::bail!("Redirect URIs cannot contain fragments: {}", uri);
    }

    Ok(())
}

fn parse_redirect_uris(input: &str) -> Vec<String> {
    input
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_scopes(input: &str) -> Vec<String> {
    input.split_whitespace().map(|s| s.to_string()).collect()
}

// ==================== OAuth2 Client Handlers ====================

#[handler]
async fn oauth2_clients_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Fetch OAuth2 clients from database
    let rows = sqlx::query!(
        "SELECT client_id, client_name, owner_user_id, redirect_uris, allowed_scopes, created
         FROM oauth2_clients
         ORDER BY created DESC"
    )
    .fetch_all(db())
    .await?;

    let clients: Vec<OAuth2ClientListItem> = rows
        .into_iter()
        .map(|row| OAuth2ClientListItem {
            client_id: row.client_id,
            client_name: row.client_name,
            owner_user_id: row.owner_user_id,
            redirect_uris: row.redirect_uris.lines().map(|s| s.to_string()).collect(),
            allowed_scopes: row
                .allowed_scopes
                .split_whitespace()
                .map(|s| s.to_string())
                .collect(),
            created: row.created,
        })
        .collect();

    let content = OAuth2ClientsListPage { clients };

    let prepared = AdminLayout::new("OAuth2 Client Management", content)
        .with_sidebar_active("engine-room-oauth2-clients")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn oauth2_client_create_page(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = OAuth2ClientCreatePage {};

    let prepared = AdminLayout::new("Create OAuth2 Client", content)
        .with_sidebar_active("engine-room-oauth2-clients")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn oauth2_client_create(
    Authenticated(user_info): Authenticated,
    form: Form<OAuth2ClientCreateForm>,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    // Validate client_id format
    validate_client_id(&form.client_id)?;

    // Parse redirect URIs and validate each one
    let redirect_uris = parse_redirect_uris(&form.redirect_uris);
    for uri in &redirect_uris {
        validate_redirect_uri(uri)?;
    }

    // Parse scopes
    let allowed_scopes = parse_scopes(&form.allowed_scopes);

    // Create new OAuth2 client
    let client = Client {
        client_id: form.client_id.clone(),
        client_secret: form.client_secret.clone(),
        client_name: form.client_name.clone(),
        redirect_uris,
        allowed_scopes,
        owner_user_id: if form.owner_user_id.is_empty() {
            None
        } else {
            Some(form.owner_user_id.clone())
        },
        created: common::timestring::TimeString::now(),
    };

    // Use LoadStorePeered to save and broadcast
    client.store().await?;

    common::service::log_activity(
        None,
        "oauth2-client-create",
        user_info.user_id,
        None,
        Some(client.client_id.clone()),
    )
    .await?;

    // Redirect to OAuth2 clients list
    Ok(poem::web::Redirect::see_other("/-admin/oauth2-clients").into_response())
}

#[handler]
async fn oauth2_client_edit_page(
    Path(client_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Load client via LoadStorePeered
    let client = Client::load(client_id).await?;

    let content = OAuth2ClientEditPage {
        client_id: client.client_id,
        client_name: client.client_name,
        client_secret: client.client_secret,
        owner_user_id: client.owner_user_id.unwrap_or_default(),
        redirect_uris: client.redirect_uris.join("\n"),
        allowed_scopes: client.allowed_scopes.join(" "),
    };

    let prepared = AdminLayout::new("Edit OAuth2 Client", content)
        .with_sidebar_active("engine-room-oauth2-clients")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn oauth2_client_update(
    Path(client_id): Path<String>,
    Authenticated(user_info): Authenticated,
    form: Form<OAuth2ClientUpdateForm>,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    // Load existing client
    let mut client = Client::load(client_id.clone()).await?;

    // Parse redirect URIs and validate each one
    let redirect_uris = parse_redirect_uris(&form.redirect_uris);
    for uri in &redirect_uris {
        validate_redirect_uri(uri)?;
    }

    // Parse scopes
    let allowed_scopes = parse_scopes(&form.allowed_scopes);

    // Update fields
    client.client_name = form.client_name.clone();
    client.client_secret = form.client_secret.clone();
    client.owner_user_id = if form.owner_user_id.is_empty() {
        None
    } else {
        Some(form.owner_user_id.clone())
    };
    client.redirect_uris = redirect_uris;
    client.allowed_scopes = allowed_scopes;

    // Use LoadStorePeered to save and broadcast
    client.store().await?;

    common::service::log_activity(
        None,
        "oauth2-client-update",
        user_info.user_id,
        None,
        Some(client_id),
    )
    .await?;

    // Redirect to OAuth2 clients list
    Ok(poem::web::Redirect::see_other("/-admin/oauth2-clients").into_response())
}

#[handler]
async fn oauth2_client_delete(
    Path(client_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let client_id_clone = client_id.clone();
    // Use LoadStorePeered to delete and broadcast to peers
    Client::delete(client_id).await?;

    common::service::log_activity(
        None,
        "oauth2-client-delete",
        user_info.user_id,
        None,
        Some(client_id_clone),
    )
    .await?;

    // Return success message for HTMX
    Ok(
        Html("<div class=\"text-green-600\">OAuth2 client deleted successfully</div>")
            .into_response(),
    )
}

// ==================== Routes ====================

pub fn oauth2_routes() -> impl Endpoint {
    Route::new()
        .at("/", get(oauth2_clients_list))
        .at(
            "/create",
            get(oauth2_client_create_page).post(oauth2_client_create),
        )
        .at(
            "/:client_id",
            get(oauth2_client_edit_page)
                .post(oauth2_client_update)
                .delete(oauth2_client_delete),
        )
}
