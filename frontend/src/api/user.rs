use crate::component::{Component, Head, StandardLayout};
use crate::middleware::authorization::Authenticated;
use anyhow::Error;
use common::message::user::{UserApiKeyRequest, UserApiKeyResponse};
use common::service::request;
use poem::{IntoResponse, Response, handler};
use sailfish::TemplateOnce;

/// Display user's API key page (user-facing, not admin)
/// GET /api/key
/// Requires authentication (handled by AuthMiddleware)
#[handler]
pub async fn api_key_page(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let user = user_info
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    // Get the user's API key from user-service
    let response: UserApiKeyResponse = request(
        "user.api_key.get",
        UserApiKeyRequest {
            user_id: user.user_id.to_string(),
        },
    )
    .await?;

    let content = ApiKeyContent {
        api_key: response.api_key,
    };

    let layout = StandardLayout::new("/api/key", content)
        .prepare(user_info.clone())
        .await?;

    let page = ApiKeyPage {
        head: Head::default().prepare(None).await?,
        layout,
    };

    Ok(page.into_response(None).await?)
}

#[derive(TemplateOnce)]
#[template(path = "api/api_key.stpl.html")]
struct ApiKeyPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<ApiKeyContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "api/api_key_content.stpl.html")]
struct ApiKeyContent {
    api_key: String,
}

/// Delete user account with confirmation
/// POST /api/delete
#[handler]
pub async fn delete_account(Authenticated(_user_info): Authenticated) -> Result<Response, Error> {
    // TODO: Implement account deletion
    // Need to:
    // 1. Delete user's entities (projects/events they own)
    // 2. Call user-service to delete user account via NATS
    // 3. Logout user (clear session)
    // 4. Redirect to homepage

    // For now, return a placeholder response
    Ok(poem::web::Html(
        r#"
    <html>
      <head>
        <title>Account Deletion Not Yet Implemented</title>
      </head>
      <body>
        <h1>Account Deletion</h1>
        <p>Account deletion is not yet implemented in this version.</p>
        <p>Please contact support for assistance.</p>
        <a href="/">Return to homepage</a>
      </body>
    </html>
    "#,
    )
    .into_response())
}
