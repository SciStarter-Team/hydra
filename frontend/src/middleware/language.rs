//! Decides what language a request is served in.
//!
//! The decision is made once, here, and put where [`crate::i18n::tr`] can
//! find it without being handed it. Everything downstream -- handlers,
//! component preparation, and the templates themselves -- just translates,
//! and none of them has to carry a language along to do it.

use poem::{Endpoint, IntoResponse, Middleware, Request, Response, session::Session};

use crate::i18n::{Lang, with_lang};

/// The query parameter and session key that pin a language.
const LANG_PARAM: &str = "lang";
const LANG_SESSION_KEY: &str = "lang";

/// The `?lang=` value that means "I have no preference -- ask my browser".
///
/// Naming a language pins it, so without this there is no way back out: even
/// `?lang=en` is a choice, and would keep English in front of a reader whose
/// browser asks for Spanish. A picker offering "use my browser's setting"
/// points at this.
pub const LANG_DEFAULT: &str = "default";

/// Whether a `?lang=` value is asking to go back to the default rather than
/// naming a language. An empty value means the same thing, so a picker whose
/// no-preference option submits blank needs no special case.
fn clears_preference(value: &str) -> bool {
    let value = value.trim();

    value.is_empty() || value.eq_ignore_ascii_case(LANG_DEFAULT)
}

pub struct LanguageMiddleware;

pub struct LanguageMiddlewareEndpoint<E> {
    wrapped: E,
}

impl<E: Endpoint> Middleware<E> for LanguageMiddleware {
    type Output = LanguageMiddlewareEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        LanguageMiddlewareEndpoint { wrapped: ep }
    }
}

impl<E: Endpoint> Endpoint for LanguageMiddlewareEndpoint<E> {
    type Output = Response;

    async fn call(&self, req: Request) -> poem::Result<Self::Output> {
        let lang = negotiate(&req);

        Ok(with_lang(lang, self.wrapped.call(req))
            .await?
            .into_response())
    }
}

/// The language to serve a request in.
///
/// In order: an explicit `?lang=`, which also sticks for the rest of the
/// session so a reader who has chosen a language keeps it while they follow
/// links; then that stored choice; then what the browser asks for; then
/// English. `?lang=default` gives up a stored choice and returns to the
/// browser's.
fn negotiate(req: &Request) -> Lang {
    let session = req.extensions().get::<Session>();

    let requested = req.uri().query().and_then(|query| {
        url::form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == LANG_PARAM)
            .map(|(_, value)| value.into_owned())
    });

    if let Some(value) = requested.as_deref() {
        if clears_preference(value) {
            if let Some(session) = session {
                session.remove(LANG_SESSION_KEY);
            }

            return from_browser(req);
        }

        if let Some(lang) = Lang::from_tag(value) {
            if let Some(session) = session {
                session.set(LANG_SESSION_KEY, lang.code());
            }

            return lang;
        }

        // A language we don't have isn't a choice, so it neither replaces a
        // stored one nor discards it. Fall through as though it weren't there.
    }

    if let Some(stored) = session
        .and_then(|session| session.get::<String>(LANG_SESSION_KEY))
        .and_then(|code| Lang::from_tag(&code))
    {
        return stored;
    }

    from_browser(req)
}

fn from_browser(req: &Request) -> Lang {
    req.headers()
        .get("Accept-Language")
        .and_then(|value| value.to_str().ok())
        .and_then(Lang::from_accept_language)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use poem::{
        EndpointExt, Route, get, handler,
        session::{CookieConfig, CookieSession},
        test::TestClient,
    };

    use super::*;
    use crate::i18n::current_lang;

    /// Reports the language the request was served in, which is the only
    /// thing the middleware is responsible for producing.
    #[handler]
    fn serving_language() -> String {
        current_lang().code().to_string()
    }

    /// The middleware over a session, as it runs in `dynamic()`.
    fn app() -> impl Endpoint {
        Route::new()
            .at("/", get(serving_language))
            .with(LanguageMiddleware)
            .with(CookieSession::new(CookieConfig::default()))
    }

    #[tokio::test]
    async fn a_language_parameter_is_recognized() {
        let response = TestClient::new(app()).get("/?lang=es").send().await;

        response.assert_status_is_ok();
        response.assert_text("es").await;
    }

    #[tokio::test]
    async fn an_explicit_choice_beats_the_browser() {
        let response = TestClient::new(app())
            .get("/?lang=pt")
            .header("Accept-Language", "es")
            .send()
            .await;

        response.assert_text("pt").await;

        // Including a choice of the language everything is written in: a
        // reader who has clicked back to English means it.
        let response = TestClient::new(app())
            .get("/?lang=en")
            .header("Accept-Language", "es")
            .send()
            .await;

        response.assert_text("en").await;
    }

    #[tokio::test]
    async fn the_browser_is_asked_when_nothing_was_chosen() {
        let response = TestClient::new(app())
            .get("/")
            .header("Accept-Language", "pt-BR,pt;q=0.9,en;q=0.8")
            .send()
            .await;

        response.assert_text("pt").await;
    }

    #[tokio::test]
    async fn a_language_we_do_not_have_falls_through() {
        let response = TestClient::new(app())
            .get("/?lang=fr")
            .header("Accept-Language", "es")
            .send()
            .await;

        response.assert_text("es").await;
    }

    #[tokio::test]
    async fn nothing_asked_for_is_english() {
        let response = TestClient::new(app()).get("/").send().await;

        response.assert_text("en").await;
    }

    /// Reads a session cookie off a response, so a later request can carry it.
    fn session_cookie(response: &poem::test::TestResponse) -> Option<String> {
        response
            .0
            .headers()
            .get("set-cookie")?
            .to_str()
            .ok()?
            .split(';')
            .next()
            .map(str::to_owned)
    }

    #[tokio::test]
    async fn a_choice_can_be_given_up() {
        let response = TestClient::new(app())
            .get(format!("/?lang={LANG_DEFAULT}"))
            .header("Accept-Language", "pt")
            .send()
            .await;

        response.assert_text("pt").await;
    }

    #[tokio::test]
    async fn an_empty_choice_means_the_same_thing() {
        let response = TestClient::new(app())
            .get("/?lang=")
            .header("Accept-Language", "pt")
            .send()
            .await;

        response.assert_text("pt").await;
    }

    /// The whole point: giving up a choice has to outlive the request that
    /// gave it up, or the next page would be back in the old language.
    #[tokio::test]
    async fn giving_up_a_choice_is_remembered_too() {
        let client = TestClient::new(app());

        let chosen = client.get("/?lang=es").send().await;
        let cookie = session_cookie(&chosen).expect("the choice is stored");

        let cleared = client
            .get(format!("/?lang={LANG_DEFAULT}"))
            .header("Cookie", &cookie)
            .header("Accept-Language", "pt")
            .send()
            .await;

        let cookie = session_cookie(&cleared).unwrap_or(cookie);
        cleared.assert_text("pt").await;

        // A later page, with no parameter at all.
        let later = client
            .get("/")
            .header("Cookie", cookie)
            .header("Accept-Language", "pt")
            .send()
            .await;

        later.assert_text("pt").await;
    }

    /// A language we don't have is a typo or a stale link, not a decision to
    /// go back to the default.
    #[tokio::test]
    async fn a_language_we_do_not_have_does_not_discard_a_choice() {
        let client = TestClient::new(app());

        let chosen = client.get("/?lang=es").send().await;
        let cookie = session_cookie(&chosen).expect("the choice is stored");

        let later = client
            .get("/?lang=fr")
            .header("Cookie", cookie)
            .header("Accept-Language", "pt")
            .send()
            .await;

        later.assert_text("es").await;
    }

    /// The point of storing it: a reader who chooses a language once keeps it
    /// while they follow links, without every link having to carry `?lang=`.
    #[tokio::test]
    async fn a_chosen_language_is_remembered() {
        let client = TestClient::new(app());

        let chosen = client.get("/?lang=es").send().await;
        let cookie = session_cookie(&chosen).expect("the choice is stored");

        // A later request, carrying the session but no parameter, and with a
        // browser that would have asked for something else.
        let later = client
            .get("/")
            .header("Cookie", cookie)
            .header("Accept-Language", "pt")
            .send()
            .await;

        later.assert_text("es").await;
    }
}
