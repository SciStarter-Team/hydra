use poem::{
    Endpoint, IntoResponse, Response, Route, get, handler,
    http::StatusCode,
    web::{Query, Redirect},
};

use crate::auth::is_local_next;
use crate::i18n::Lang;
use crate::middleware::authorization::Authenticated;

pub fn routes() -> impl Endpoint {
    Route::new().at("/ensure-account", get(ensure_account))
}

#[derive(Debug, serde::Deserialize)]
struct EnsureAccountParams {
    next: String,
    lang: Option<String>,
}

/// Sends a visitor on to `next`, by way of the sign-up form if they are not
/// signed in. A training module's last page links here, through a go link
/// that carries the module's completion, to have a learner create their
/// account and come back to the training page to find the badge awarded:
/// the go link holds the completion in the session of a visitor with no
/// account, and signing up applies it. A learner who is already signed in
/// had it recorded by the go link, and is sent straight on.
///
/// `lang` picks the language of the sign-up form, for a module in another
/// language. Orrery serves the same path, since the go links that lead here
/// name it.
#[handler]
async fn ensure_account(
    Authenticated(user_info): Authenticated,
    Query(params): Query<EnsureAccountParams>,
) -> Response {
    if !is_local_next(&params.next) {
        return (StatusCode::BAD_REQUEST, "next must be a path on this site").into_response();
    }

    if user_info.is_some() {
        return Redirect::see_other(&params.next).into_response();
    }

    Redirect::see_other(signup_url(&params.next, params.lang.as_deref())).into_response()
}

/// The sign-up form, returning to `next`, in the language `lang` names if it
/// names one the site has.
fn signup_url(next: &str, lang: Option<&str>) -> String {
    let mut url = format!("/login?next={}", urlencoding::encode(next));

    if let Some(lang) = lang.and_then(Lang::from_tag) {
        url.push_str("&lang=");
        url.push_str(lang.code());
    }

    url
}

#[cfg(test)]
mod tests {
    use super::signup_url;

    /// The return path survives as one parameter, fragment included, rather
    /// than ending the query at its `#`.
    #[test]
    fn the_return_path_is_carried_whole() {
        assert_eq!(
            signup_url("/training-foundations#start", None),
            "/login?next=%2Ftraining-foundations%23start"
        );
    }

    #[test]
    fn a_language_the_site_has_is_passed_on() {
        assert_eq!(
            signup_url("/training-foundations#start", Some("es")),
            "/login?next=%2Ftraining-foundations%23start&lang=es"
        );
        assert_eq!(signup_url("/x", Some("fr")), "/login?next=%2Fx");
    }
}
