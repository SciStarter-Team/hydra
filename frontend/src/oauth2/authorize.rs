// OAuth2 Authorization Endpoint

use poem::{handler, Response, IntoResponse, web::{Query, Form, Redirect}};
use anyhow::Error;
use serde::Deserialize;
use sailfish::TemplateOnce;
use crate::middleware::authorization::Authenticated;
use crate::oauth2::types::{Client, AuthorizationCode};
use crate::peering::LoadStorePeered;
use crate::component::{Component, Head, StandardLayout};

#[derive(Debug, Deserialize)]
pub struct AuthorizeQuery {
    client_id: String,
    redirect_uri: String,
    response_type: String,
    scope: Option<String>,
    state: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuthorizeForm {
    client_id: String,
    redirect_uri: String,
    scope: String,
    state: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    approve: Option<String>,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "oauth2/consent_page.stpl.html")]
struct ConsentPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<ConsentContent> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "oauth2/consent.stpl.html")]
struct ConsentContent {
    client_name: String,
    client_id: String,
    redirect_uri: String,
    scope: String,
    state: String,
    code_challenge: String,
    code_challenge_method: String,
}

/// GET /oauth2/authorize
/// Shows consent screen for OAuth2 authorization
#[handler]
pub async fn authorize_get(
    Query(query): Query<AuthorizeQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    // Require authentication
    let user = match user_info {
        Some(user) => user,
        None => {
            // Redirect to login, then back to this authorization request
            let login_url = format!(
                "/login?next={}",
                urlencoding::encode(&format!(
                    "/oauth2/authorize?client_id={}&redirect_uri={}&response_type={}&scope={}&state={}",
                    urlencoding::encode(&query.client_id),
                    urlencoding::encode(&query.redirect_uri),
                    urlencoding::encode(&query.response_type),
                    urlencoding::encode(&query.scope.as_deref().unwrap_or("openid")),
                    urlencoding::encode(&query.state.as_deref().unwrap_or(""))
                ))
            );
            return Ok(Redirect::see_other(login_url).into_response());
        }
    };

    // Validate response_type
    if query.response_type != "code" {
        return error_redirect(&query.redirect_uri, "unsupported_response_type", query.state.as_deref());
    }

    // Load and validate client
    let client = match Client::load(query.client_id.clone()).await {
        Ok(client) => client,
        Err(_) => {
            return error_redirect(&query.redirect_uri, "invalid_client", query.state.as_deref());
        }
    };

    // Verify redirect_uri
    if !client.verify_redirect_uri(&query.redirect_uri) {
        return Err(anyhow::anyhow!("Invalid redirect_uri for client"));
    }

    // Verify scope
    let scope = query.scope.as_deref().unwrap_or("openid");
    if !client.verify_scope(scope) {
        return error_redirect(&query.redirect_uri, "invalid_scope", query.state.as_deref());
    }

    // Render consent screen
    let content = ConsentContent {
        client_name: client.client_name,
        client_id: query.client_id,
        redirect_uri: query.redirect_uri,
        scope: scope.to_string(),
        state: query.state.unwrap_or_default(),
        code_challenge: query.code_challenge.unwrap_or_default(),
        code_challenge_method: query.code_challenge_method.unwrap_or_default(),
    };

    let layout = StandardLayout::new(content).prepare(Some(user.clone())).await?;
    let head = Head::default();

    let page = ConsentPage { head, layout };
    let html = page.render_once()?;

    Ok(poem::web::Html(html).into_response())
}

/// POST /oauth2/authorize
/// Processes consent and issues authorization code
#[handler]
pub async fn authorize_post(
    Form(form): Form<AuthorizeForm>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    // Require authentication
    let user = match user_info {
        Some(user) => user,
        None => {
            return Ok(Redirect::see_other("/login").into_response());
        }
    };

    // Check if user approved
    if form.approve.as_deref() != Some("yes") {
        return error_redirect(&form.redirect_uri, "access_denied", form.state.as_deref());
    }

    // Load and validate client
    let client = match Client::load(form.client_id.clone()).await {
        Ok(client) => client,
        Err(_) => {
            return error_redirect(&form.redirect_uri, "invalid_client", form.state.as_deref());
        }
    };

    // Verify redirect_uri
    if !client.verify_redirect_uri(&form.redirect_uri) {
        return Err(anyhow::anyhow!("Invalid redirect_uri for client"));
    }

    // Verify scope
    if !client.verify_scope(&form.scope) {
        return error_redirect(&form.redirect_uri, "invalid_scope", form.state.as_deref());
    }

    // Generate authorization code
    let auth_code = AuthorizationCode::new(
        form.client_id.clone(),
        user.user_id.to_string(),
        form.redirect_uri.clone(),
        form.scope.clone(),
        form.code_challenge.clone(),
        form.code_challenge_method.clone(),
    );

    // Store authorization code with LoadStorePeered for distributed sync
    auth_code.store().await?;

    // Build redirect URL with code and state
    let mut redirect_url = form.redirect_uri.clone();
    redirect_url.push_str(if redirect_url.contains('?') { "&" } else { "?" });
    redirect_url.push_str(&format!("code={}", urlencoding::encode(&auth_code.code)));

    if let Some(state) = form.state {
        redirect_url.push_str(&format!("&state={}", urlencoding::encode(&state)));
    }

    Ok(Redirect::see_other(redirect_url).into_response())
}

/// Helper to redirect with OAuth2 error
fn error_redirect(redirect_uri: &str, error: &str, state: Option<&str>) -> Result<Response, Error> {
    let mut url = redirect_uri.to_string();
    url.push_str(if url.contains('?') { "&" } else { "?" });
    url.push_str(&format!("error={}", urlencoding::encode(error)));

    if let Some(state) = state {
        url.push_str(&format!("&state={}", urlencoding::encode(state)));
    }

    Ok(Redirect::see_other(url).into_response())
}
