//! Admin management of content-management MCP tokens (Engine Room).
//!
//! Tokens authorize access to the `/-content-mcp` endpoint (see
//! [`crate::mcp::content`]). The secret is shown once, immediately after
//! creation; thereafter only a short preview is displayed. Tokens are
//! revoked by deletion.

use std::sync::Arc;

use crate::component::{AdminLayout, Component};
use crate::mcp_token::McpToken;
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use common::message::user::BasicInfo;
use poem::{
    Endpoint, Route, delete, get, handler, post,
    web::{Form, Path},
};
use sailfish::TemplateOnce;
use serde::Deserialize;

// ==================== Templates ====================

struct TokenListItem {
    /// The full secret, used in the delete control. Not displayed.
    token: String,
    /// A short, non-recoverable preview for the listing.
    preview: String,
    name: String,
    created: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/mcp_tokens.stpl.html")]
struct McpTokensPage {
    tokens: Vec<TokenListItem>,
    /// The freshly minted secret, shown once after creation.
    new_token: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateTokenForm {
    name: String,
}

// ==================== Handlers ====================

async fn render_tokens_page(
    user_info: Option<Arc<BasicInfo>>,
    new_token: Option<String>,
    error_message: Option<String>,
) -> Result<poem::Response, anyhow::Error> {
    let tokens = McpToken::list()
        .await?
        .into_iter()
        .map(|token| {
            let preview = format!("{}…", &token.token[..token.token.len().min(12)]);
            TokenListItem {
                token: token.token,
                preview,
                name: token.name,
                created: token.created.to_string(),
            }
        })
        .collect();

    let content = McpTokensPage {
        tokens,
        new_token,
        error_message,
    };

    let prepared = AdminLayout::new("MCP Tokens", content)
        .with_sidebar_active("engine-room-mcp-tokens")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("MCP Tokens", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn tokens_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    render_tokens_page(user_info, None, None).await
}

#[handler]
async fn token_create(
    Form(form): Form<CreateTokenForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let name = form.name.trim();
    if name.is_empty() {
        return render_tokens_page(user_info, None, Some("A name is required".to_string())).await;
    }

    let token = McpToken::new(name);
    token.store().await?;

    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            "mcp-token-create",
            user_info.user_id,
            None,
            Some(name.to_string()),
        )
        .await?;
    }

    render_tokens_page(user_info, Some(token.token), None).await
}

#[handler]
async fn token_delete(
    Path(token): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    McpToken::delete(token).await?;

    if let Some(user_info) = &user_info {
        common::service::log_activity(None, "mcp-token-delete", user_info.user_id, None, None)
            .await?;
    }

    // The row removes itself via an empty outerHTML swap.
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(String::new()))
}

// ==================== Routes ====================

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(tokens_list))
        .at("/create", post(token_create))
        .at("/:token", delete(token_delete))
}
