use common::{
    message::{
        location::LocationInfo,
        user::{BasicInfo, Login, Signup},
    },
    service::{log_activity, request},
};
use poem::{
    IntoResponse, Response, handler,
    session::Session,
    web::{Form, Query, RealIp, Redirect},
};

use crate::{
    component::{Component, Footer, Head, Header},
    middleware::authorization::Authenticated,
};

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
    head: <Head as Component>::Prepared,
    header: <Header as Component>::Prepared,
    footer: <Footer as Component>::Prepared,
}

#[handler]
pub async fn login_signup_page(
    session: &Session,
    Authenticated(user_info): Authenticated,
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
        head: Head::default().prepare(None).await?,
        header: Header::default().prepare(user_info.clone()).await?,
        footer: Footer::default().prepare(user_info).await?,
    }
    .render(None)
    .await
}

#[derive(serde::Deserialize)]
struct SignupForm {
    handle: String,
    email: String,
    password1: String,
    password2: String,
    newsletter: Option<bool>,
    zipcode: String,
}

#[handler]
pub async fn signup_action(
    session: &Session,
    RealIp(client_ip): RealIp,
    Form(form): Form<SignupForm>,
) -> Response {
    let next: Option<String> = session.get("auth_next");

    // Validate password confirmation
    if form.password1 != form.password2 {
        return Redirect::see_other("/login?password=mismatch").into_response();
    }

    // Get session values for protected and tags
    let protected = session.get::<bool>("signup_protected").unwrap_or(false);
    let tags = session
        .get::<Vec<String>>("signup_tags")
        .unwrap_or_default();

    // Get user location from location service
    let point: Option<String> = if let Some(addr) = client_ip {
        match request::<LocationInfo>("location.ip", addr.to_string()).await {
            Ok(location) => location.as_point().map(|point| point.to_string()),
            Err(_) => None,
        }
    } else {
        None
    };

    let signup_request = Signup {
        handle: form.handle,
        email: form.email,
        password: form.password1,
        newsletter: form.newsletter.unwrap_or(false),
        zipcode: form.zipcode,
        protected,
        point,
        tags,
    };

    // Call user-service's signup via NATS
    match request::<Option<BasicInfo>>("user.auth.signup", signup_request).await {
        Ok(Some(basic_info)) => {
            // Store user info in session
            session.set("basic_info", &basic_info);

            // Clear signup-specific session data
            session.remove("signup_protected");
            session.remove("signup_tags");

            // Redirect to intended page or home
            if let Some(next) = next {
                session.remove("auth_next");
                Redirect::see_other(next).into_response()
            } else {
                Redirect::see_other("/").into_response()
            }
        }
        Ok(None) => {
            // Signup failed - redirect back with error
            Redirect::see_other("/login?signup=failed").into_response()
        }
        Err(_) => {
            // Service error
            Redirect::see_other("/login?signup=error").into_response()
        }
    }
}

#[derive(serde::Deserialize)]
struct LoginForm {
    email: String,
    password: String,
}

#[handler]
pub async fn login_action(session: &Session, Form(form): Form<LoginForm>) -> Response {
    let next: Option<String> = session.get("auth_next");

    let login_request = Login {
        email: form.email.clone(),
        password: form.password,
    };

    // Call user-service's login via NATS
    match request::<Option<BasicInfo>>("user.auth.login", login_request).await {
        Ok(Some(basic_info)) => {
            // Store user info in session
            session.set("basic_info", &basic_info);

            // Redirect to intended page or home
            if let Some(next) = next {
                session.remove("auth_next");
                Redirect::see_other(next).into_response()
            } else {
                Redirect::see_other("/").into_response()
            }
        }
        Ok(None) => {
            // Login failed - redirect back with error
            Redirect::see_other(format!(
                "/login?username={}&password=true",
                urlencoding::encode(&form.email)
            ))
            .into_response()
        }
        Err(_) => {
            // Service error
            Redirect::see_other(format!(
                "/login?username={}&error=true",
                urlencoding::encode(&form.email)
            ))
            .into_response()
        }
    }
}

#[handler]
pub async fn logout_action(session: &Session) -> impl IntoResponse {
    let Some(info): Option<BasicInfo> = session.get("basic_info") else {
        return Redirect::see_other("/");
    };

    session.remove("basic_info");

    log_activity(None, "logout", info.user_id, None, None)
        .await
        .ok();

    Redirect::see_other("/")
}
