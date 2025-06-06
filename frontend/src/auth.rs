use poem::{
    IntoResponse, Response, handler,
    session::Session,
    web::{Form, Query, Redirect},
};

use crate::component::Component;

#[derive(serde::Deserialize)]
struct LoginPageParameters {
    username: Option<bool>,
    password: Option<bool>,
    unauthorized: Option<bool>,
    next: Option<String>,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "auth/login_signup.stpl.html")]
struct LoginSignupPage {
    username: bool,
    password: bool,
    unauthorized: bool,
}

#[handler]
pub async fn login_signup_page(
    session: &Session,
    Query(params): Query<LoginPageParameters>,
) -> Result<Response, anyhow::Error> {
    if let Some(next) = params.next {
        // Absolute local paths only for next values. No relative, no off-site
        if next.starts_with("/") {
            session.set("auth_next", next);
        }
    }

    LoginSignupPage {
        username: params.username.unwrap_or_default(),
        password: params.password.unwrap_or_default(),
        unauthorized: params.unauthorized.unwrap_or_default(),
    }
    .render(None)
    .await
}

#[derive(serde::Deserialize)]
struct SignupForm {}

#[handler]
pub async fn signup_action(session: &Session, Form(form): Form<SignupForm>) -> Response {
    let next: Option<String> = session.get("auth_next");

    if let Some(next) = next {
        session.remove("auth_next");
        Redirect::see_other(next).into_response()
    } else {
        Redirect::see_other("/").into_response()
    }
}

#[derive(serde::Deserialize)]
struct LoginForm {}

#[handler]
pub async fn login_action(session: &Session, Form(form): Form<LoginForm>) -> Response {
    let next: Option<String> = session.get("auth_next");

    if let Some(next) = next {
        session.remove("auth_next");
        Redirect::see_other(next).into_response()
    } else {
        Redirect::see_other("/").into_response()
    }
}

#[handler]
pub async fn logout_action(session: &Session) -> impl IntoResponse {
    session.remove("basic_info");
    Redirect::see_other("/")
}
