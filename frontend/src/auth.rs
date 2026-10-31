use common::{
    badge::Badge,
    message::{
        location::LocationInfo,
        user::{
            BasicInfo, CompleteSignup, CompleteSignupResponse, DeferSignup, DeferSignupOutcome,
            Login, Signup, UserAndBadge, WorkflowProgressRequest, WorkflowProgressResponse,
            signup_needs_protection,
        },
        webauthn::{
            WebAuthnAuthenticationCompleteRequest, WebAuthnAuthenticationCompleteResponse,
            WebAuthnAuthenticationStartRequest, WebAuthnAuthenticationStartResponse,
            WebAuthnRegistrationCompleteRequest, WebAuthnRegistrationCompleteResponse,
            WebAuthnRegistrationStartRequest, WebAuthnRegistrationStartResponse,
        },
    },
    redirect::is_local_next,
    service::{log_activity, request, request_with_retry, send},
};
use poem::{
    Endpoint, EndpointExt, IntoResponse, Response, Route, handler, post,
    session::Session,
    web::{Form, Json, Query, RealIp, Redirect},
};
use rand::{Rng, thread_rng};
use std::time::Duration;
use ulid::Ulid;

use crate::{
    component::{Component, Head, StandardLayout},
    middleware::{
        attribution::first_touch,
        authorization::Authenticated,
        rate_limit::{RateLimitConfig, RateLimitMiddleware},
    },
    session::{pending_transitions, session_tags},
};

/// Look at the badge a tracked go link staked out for this visitor, without
/// spending it.
///
/// Deliberately a peek: a signup that fails because the address already has an
/// account sends the visitor to log in instead, and the badge has to still be
/// there when they do. Only [`clear_staked_out_badge`] gives it up, once
/// something that will see it through is holding it.
///
/// A badgeclass this build has never heard of is dropped on sight — that is a
/// misconfigured link, and keeping it would just log on every later attempt.
fn staked_out_badge(session: &Session) -> Option<Badge> {
    let badgeclass: String = session.get("grant-badge")?;

    let badge = Badge::from_badgeclass(&badgeclass);

    if badge.is_none() {
        tracing::error!("Go link staked out unknown badgeclass {badgeclass:?}");
        session.remove("grant-badge");
    }

    badge
}

/// Give up the staked-out badge, once it has been handed to something that
/// will see it through: an account that now exists, or a pending row that
/// outlives this session.
fn clear_staked_out_badge(session: &Session) {
    session.remove("grant-badge");
}

/// Hand the session's signup-context tags to an account that already exists.
///
/// Sent as signup tags rather than ordinary ones: these arrived on the link
/// the person followed, which is what decides whether Constant Contact is
/// allowed to see a name no allowlist could have predicted -- and what a
/// partner's cohort tag is recognized by on the receiving end, where a tag
/// that credits its cohort with a training module credits it.
///
/// Drained rather than read, so the tags land once. The signup lanes pass the
/// same list to `create_account` instead, which writes it into the row that
/// creates the user.
async fn apply_session_tags(session: &Session, user_id: Ulid) {
    for tag in session_tags::clear(session) {
        if let Err(e) = send("user.tags.add_signup", (user_id.to_string(), tag)).await {
            tracing::error!("Failed to record a signup tag for {user_id}: {e}");
        }
    }
}

/// Grant a staked-out badge to someone who turned out to already have an
/// account. The signup lanes hand it to `create_account` instead, which grants
/// it from the row that creates the user.
async fn grant_staked_out_badge(session: &Session, user_id: Ulid) {
    let Some(badge) = staked_out_badge(session) else {
        return;
    };

    clear_staked_out_badge(session);

    if let Err(e) = send("user.badges.grant", UserAndBadge { user_id, badge }).await {
        tracing::error!("Failed to grant staked-out badge to {user_id}: {e}");
    }
}

/// Record the training steps held in the session on someone who turned out
/// to already have an account. The signup lanes hand them to `create_account`
/// instead, which records them from the row that creates the user.
///
/// Released first and recorded after: once the account is in hand there is
/// nothing left for the session to hold them for, and a step the user service
/// cannot record right now is logged rather than kept to be retried on every
/// later sign-in.
async fn apply_pending_transitions(session: &Session, user_id: Ulid) {
    for held in pending_transitions::clear(session) {
        let recorded: Result<WorkflowProgressResponse, _> = request(
            "user.workflow.progress",
            WorkflowProgressRequest {
                user_id: user_id.to_string(),
                workflow: held.workflow,
                step: held.step.clone(),
            },
        )
        .await;

        if let Err(e) = recorded {
            tracing::error!(
                "Failed to record the {} step {:?} held for {user_id}: {e}",
                held.workflow.slug(),
                held.step
            );
        }
    }
}

/// Generate a random innocuous display name to avoid exposing email addresses
fn generate_random_display_name() -> String {
    let adjectives = [
        "Happy",
        "Curious",
        "Bright",
        "Friendly",
        "Creative",
        "Helpful",
        "Clever",
        "Gentle",
        "Kind",
        "Smart",
        "Brave",
        "Calm",
        "Quick",
        "Cool",
        "Bold",
        "Wise",
        "Swift",
        "Noble",
        "Keen",
        "Sharp",
        "Clear",
        "Fresh",
        "Pure",
        "Active",
        "Alert",
        "Amazing",
        "Awesome",
        "Bouncy",
        "Cheerful",
        "Cosmic",
        "Daring",
        "Dynamic",
        "Eager",
        "Elegant",
        "Epic",
        "Exciting",
        "Famous",
        "Fantastic",
        "Fearless",
        "Fierce",
        "Flying",
        "Fortunate",
        "Funny",
        "Gigantic",
        "Glorious",
        "Golden",
        "Graceful",
        "Grand",
        "Great",
        "Groovy",
        "Handy",
        "Hardy",
        "Heroic",
        "Honest",
        "Hopeful",
        "Humble",
        "Impressive",
        "Inspiring",
        "Intelligent",
        "Joyful",
        "Jumping",
        "Lively",
        "Lucky",
        "Magical",
        "Majestic",
        "Marvelous",
        "Mighty",
        "Nimble",
        "Optimistic",
        "Outstanding",
        "Peaceful",
        "Perfect",
        "Playful",
        "Pleasant",
        "Polite",
        "Positive",
        "Powerful",
        "Proud",
        "Radiant",
        "Rapid",
        "Remarkable",
        "Resourceful",
        "Shining",
        "Sparkling",
        "Special",
        "Speedy",
        "Spirited",
        "Stellar",
        "Strong",
        "Stunning",
        "Super",
        "Talented",
        "Terrific",
        "Thoughtful",
        "Thrilling",
        "Tremendous",
        "Trusty",
        "Ultimate",
        "Unique",
        "Vibrant",
        "Victorious",
        "Vivid",
        "Wonderful",
        "Zesty",
        "Zippy",
    ];

    let animals = [
        "Penguin",
        "Dolphin",
        "Owl",
        "Fox",
        "Rabbit",
        "Cat",
        "Dog",
        "Bird",
        "Turtle",
        "Bee",
        "Butterfly",
        "Fish",
        "Duck",
        "Deer",
        "Bear",
        "Lion",
        "Tiger",
        "Eagle",
        "Hawk",
        "Wolf",
        "Seal",
        "Whale",
        "Frog",
        "Squirrel",
        "Albatross",
        "Antelope",
        "Badger",
        "Beaver",
        "Bison",
        "Bluebird",
        "Bobcat",
        "Buffalo",
        "Cardinal",
        "Caribou",
        "Cheetah",
        "Chipmunk",
        "Condor",
        "Coyote",
        "Crane",
        "Cricket",
        "Dragonfly",
        "Elk",
        "Falcon",
        "Ferret",
        "Firefly",
        "Flamingo",
        "Gazelle",
        "Giraffe",
        "Goose",
        "Gorilla",
        "Heron",
        "Hippo",
        "Hummingbird",
        "Iguana",
        "Jackal",
        "Jaguar",
        "Jellyfish",
        "Kangaroo",
        "Koala",
        "Ladybug",
        "Lemur",
        "Leopard",
        "Llama",
        "Lobster",
        "Lynx",
        "Manatee",
        "Mantis",
        "Meerkat",
        "Moose",
        "Narwhal",
        "Newt",
        "Octopus",
        "Opossum",
        "Orca",
        "Otter",
        "Panda",
        "Panther",
        "Parrot",
        "Peacock",
        "Pelican",
        "Phoenix",
        "Platypus",
        "Porcupine",
        "Puffin",
        "Quail",
        "Raccoon",
        "Raven",
        "Rhino",
        "Robin",
        "Salamander",
        "Seahorse",
        "Shark",
        "Sheep",
        "Sloth",
        "Snail",
        "Sparrow",
        "Starfish",
        "Stingray",
        "Swan",
        "Toucan",
        "Trout",
        "Walrus",
        "Weasel",
        "Woodpecker",
        "Zebra",
        "Chipmunk",
        "Hamster",
        "Hedgehog",
        "Mongoose",
        "Armadillo",
        "Chimpanzee",
    ];

    let mut rng = thread_rng();
    let adjective = adjectives[rng.gen_range(0..adjectives.len())];
    let animal = animals[rng.gen_range(0..animals.len())];
    let number = rng.gen_range(1..1000);

    format!("{}{}{}", adjective, animal, number)
}

#[derive(Debug, serde::Deserialize)]
struct LoginPageParameters {
    system: Option<bool>,
    password: Option<bool>,
    mismatch: Option<bool>,
    unauthorized: Option<bool>,
    exists: Option<bool>,
    ratelimited: Option<bool>,
    next: Option<String>,
    page: Option<String>,
    tag: Option<String>,
}

/// The site the `page=` return parameter can send a visitor back to.
const PAGES_ORIGIN: &str = "https://pages.scistarter.org";

/// Build the post-auth return URL for a `page=` value, or None if unusable.
///
/// `page=` is the pages.scistarter.org counterpart to `next=`: the caller
/// supplies a path only and the origin is supplied here, so the destination
/// cannot be aimed at another host. Keeping the two parameters separate is
/// what makes that structural rather than dependent on validation.
pub fn pages_return_url(page: &str) -> Option<String> {
    is_local_next(page).then(|| format!("{PAGES_ORIGIN}{page}"))
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "auth/login_signup.stpl.html")]
struct LoginSignupPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<LoginSignupContent> as Component>::Prepared,
}

/// Whether an account signing up from this session would be protected.
///
/// Both halves of the answer, because the signup form has to show the visitor
/// what they are actually going to get: the flag a protected signup context
/// sets directly, and the tag rule that user-service applies to whatever tags
/// ride along. `signup_action` passes only the flag, leaving the tag half to
/// user-service's own door, so this is the one place that has to combine them.
pub fn signup_will_be_protected(session: &Session) -> bool {
    session.get::<bool>("signup_protected").unwrap_or(false)
        || signup_needs_protection(&session_tags::list(session))
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "auth/login_signup_content.stpl.html")]
struct LoginSignupContent {
    system: bool,
    password: bool,
    mismatch: bool,
    unauthorized: bool,
    exists: bool,
    ratelimited: bool,
    turnstile_sitekey: String,
    /// Cloudflare's own language tag, which is not always ours: Brazilian
    /// Portuguese is the only Portuguese Turnstile offers.
    turnstile_language: &'static str,
    /// A protected account is never added to a mailing list, so its owner is
    /// told what they are signing up for rather than offered a newsletter
    /// opt-in that user-service is going to discard.
    protected: bool,
}

#[handler]
#[tracing::instrument]
pub async fn login_signup_page(
    session: &Session,
    Authenticated(user_info): Authenticated,
    Query(params): Query<LoginPageParameters>,
) -> Result<Response, anyhow::Error> {
    if let Some(next) = params.next {
        // Absolute local paths only for next values. No relative, no off-site
        if is_local_next(&next) {
            session.set("auth_next", next);
        }
    }

    // The blog embeds our header cross-origin and sends visitors here to log
    // in; `page=` carries the path to return them to. See [pages_return_url]
    // for why it is a separate parameter from `next`.
    match params.page {
        Some(page) if is_local_next(&page) => session.set("auth_page", page),
        // Reaching the login page from anywhere else ends any pending return
        // to the blog, so a later unrelated login does not send the visitor
        // somewhere they have long since navigated away from.
        _ => session.remove("auth_page"),
    }

    if let Some(tag) = params.tag {
        // Stash the tag in the session so it is applied to the account on
        // signup (via Signup.tags) or on login into an existing account (via
        // user.tags.add_signup). Either way it reaches the rules a partner's
        // cohort tag decides -- protection, and the training credit that goes
        // with a module the cohort ran off-site. Empty values are ignored.
        let tag = tag.trim();
        if !tag.is_empty() {
            session_tags::add(session, tag);
        }
    }

    let layout = StandardLayout::new(
        "/login",
        LoginSignupContent {
            system: params.system.unwrap_or_default(),
            password: params.password.unwrap_or_default(),
            mismatch: params.mismatch.unwrap_or_default(),
            unauthorized: params.unauthorized.unwrap_or_default(),
            exists: params.exists.unwrap_or_default(),
            ratelimited: params.ratelimited.unwrap_or_default(),
            turnstile_sitekey: env!("TURNSTILE_SITEKEY").to_string(),
            turnstile_language: match crate::i18n::current_lang() {
                crate::i18n::Lang::Es => "es",
                crate::i18n::Lang::Pt => "pt-BR",
                crate::i18n::Lang::En => "en",
            },
            // Read after the `?tag=` above has been stashed, so a link that
            // both names a protected cohort and lands on this page is
            // recognized on that first render rather than the next one.
            protected: signup_will_be_protected(session),
        },
    )
    .prepare(user_info.clone())
    .await?;

    LoginSignupPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(Debug, serde::Deserialize)]
struct SignupForm {
    handle: String,
    email: String,
    password1: String,
    password2: String,
    newsletter: Option<bool>,
    zipcode: String,
    use_passkey: Option<bool>,
    #[serde(rename = "cf-turnstile-response")]
    cf_turnstile_response: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct TurnstileVerifyResponse {
    success: bool,
}

async fn verify_turnstile(token: &str, remote_ip: Option<std::net::IpAddr>) -> bool {
    let mut params = vec![
        ("secret", env!("TURNSTILE_SECRET").to_string()),
        ("response", token.to_string()),
    ];
    if let Some(ip) = remote_ip {
        params.push(("remoteip", ip.to_string()));
    }
    match reqwest::Client::new()
        .post("https://challenges.cloudflare.com/turnstile/v0/siteverify")
        .form(&params)
        .send()
        .await
    {
        Ok(resp) => match resp.json::<TurnstileVerifyResponse>().await {
            Ok(body) => body.success,
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// The fields a password signup supplies; everything else about the account
/// comes from the session.
pub struct PasswordSignup {
    pub handle: String,
    pub email: String,
    pub password: String,
    pub newsletter: bool,
    pub zipcode: String,
    /// The project to join, if the signup was made from one's page.
    pub join_project: Option<String>,
}

/// How a password signup came out.
pub enum SignupOutcome {
    /// The account exists and the session is signed in to it.
    SignedIn(BasicInfo),
    /// Turnstile couldn't vouch for the visitor, so the account waits on a
    /// link emailed to them.
    Deferred,
    /// The email address already belongs to an account.
    AlreadyRegistered,
    /// Too many deferred signups from the visitor's network.
    RateLimited,
    /// Something on our side failed.
    Unavailable,
}

/// Create an account with a password, the way every password signup form
/// does: protected status, tags, a staked-out badge, held training steps and
/// attribution all come from the session, and the account is signed in.
///
/// `turnstile_token` is what the form's Turnstile challenge produced, empty
/// if nothing. Without its vouching the signup is deferred behind an emailed
/// link rather than refused, and `next` is where that link lands.
pub async fn sign_up_with_password(
    session: &Session,
    client_ip: Option<std::net::IpAddr>,
    fields: PasswordSignup,
    turnstile_token: &str,
    next: Option<String>,
) -> SignupOutcome {
    let turnstile_ok =
        !turnstile_token.is_empty() && verify_turnstile(turnstile_token, client_ip).await;

    // Get session values for protected and tags
    let protected = session.get::<bool>("signup_protected").unwrap_or(false);
    let tags = session_tags::list(session);

    // Get user location from location service
    let point: Option<String> = if let Some(addr) = client_ip {
        match request::<LocationInfo>("location.ip", addr.to_string()).await {
            Ok(location) => location.as_point().map(|point| point.to_string()),
            Err(_) => None,
        }
    } else {
        None
    };

    let email = fields.email.clone();

    let signup_request = Signup {
        handle: fields.handle,
        email: fields.email,
        password: fields.password,
        newsletter: fields.newsletter,
        zipcode: fields.zipcode,
        protected,
        point,
        tags,
        disabled_password: false, // Regular password account
        badgeclass: staked_out_badge(session),
        attribution: first_touch(session),
        pending_transitions: pending_transitions::list(session),
        join_project: fields.join_project,
    };

    if !turnstile_ok {
        // Turnstile never produced a token (blocked script, filtered
        // network) or its verdict was negative. Neither proves the
        // visitor is a bot, so instead of rejecting them the signup is
        // deferred behind an emailed completion link: a human gets in
        // with one click, a bot without a real inbox gets nothing.
        let deferral = DeferSignup {
            signup: signup_request,
            next,
            ip_address: client_ip.map(|ip| ip.to_string()),
        };

        return match request::<DeferSignupOutcome>("user.auth.defer_signup", deferral).await {
            Ok(DeferSignupOutcome::Deferred) => {
                session.set("pending_signup_email", &email);
                // The pending row holds the badge and the held steps
                // now, and it outlives this session — the completion
                // link may be clicked in another browser entirely.
                clear_staked_out_badge(session);
                pending_transitions::clear(session);
                SignupOutcome::Deferred
            }
            Ok(DeferSignupOutcome::AlreadyRegistered) => SignupOutcome::AlreadyRegistered,
            Ok(DeferSignupOutcome::RateLimited) => SignupOutcome::RateLimited,
            Err(_) => SignupOutcome::Unavailable,
        };
    }

    // Call user-service's signup via NATS
    match request_with_retry::<Option<BasicInfo>>("user.auth.signup", signup_request).await {
        Ok(Some(basic_info)) => {
            // Regular password signup - log them in immediately.
            // Rotate the session ID on this privilege transition to
            // prevent session fixation.
            session.renew();
            session.set("basic_info", &basic_info);

            // Clear signup-specific session data
            session.remove("signup_protected");
            session_tags::clear(session);
            clear_staked_out_badge(session);
            pending_transitions::clear(session);

            log_activity(None, "user_signup", basic_info.user_id, None, next)
                .await
                .ok();

            SignupOutcome::SignedIn(basic_info)
        }
        // The email address already belongs to an account.
        Ok(None) => SignupOutcome::AlreadyRegistered,
        Err(_) => SignupOutcome::Unavailable,
    }
}

/// Where a visitor goes once signed in, by the `page=` or `next=` the login
/// page was given, and forgets them.
fn post_auth_redirect(session: &Session, next: Option<String>) -> Response {
    let page_url = session
        .get::<String>("auth_page")
        .as_deref()
        .and_then(pages_return_url);
    if let Some(page_url) = page_url {
        session.remove("auth_page");
        session.remove("auth_next");
        Redirect::see_other(page_url).into_response()
    } else if let Some(next) = next {
        session.remove("auth_next");
        Redirect::see_other(next).into_response()
    } else {
        Redirect::see_other("/").into_response()
    }
}

#[handler]
#[tracing::instrument]
pub async fn signup_action(
    session: &Session,
    RealIp(client_ip): RealIp,
    Form(form): Form<SignupForm>,
) -> Response {
    let next: Option<String> = session
        .get::<String>("auth_next")
        .filter(|n| is_local_next(n));

    // For passkey users, this shouldn't be reached since JavaScript handles the flow
    // This is only for password users now
    if form.use_passkey.unwrap_or(false) {
        // Passkey signup - this shouldn't be reached since JavaScript handles it
        // Return an error if somehow we get here
        return Redirect::see_other("/login?system=true").into_response();
    }

    // Validate password confirmation
    if form.password1 != form.password2 {
        return Redirect::see_other("/login?mismatch=true").into_response();
    }

    let token = form.cf_turnstile_response.as_deref().unwrap_or("");

    let fields = PasswordSignup {
        handle: form.handle,
        email: form.email,
        password: form.password1,
        newsletter: form.newsletter.unwrap_or(false),
        zipcode: form.zipcode,
        join_project: None,
    };

    match sign_up_with_password(session, client_ip, fields, token, next.clone()).await {
        SignupOutcome::SignedIn(_) => post_auth_redirect(session, next),
        SignupOutcome::Deferred => Redirect::see_other("/check-email").into_response(),
        SignupOutcome::AlreadyRegistered => {
            Redirect::see_other("/login?exists=true").into_response()
        }
        SignupOutcome::RateLimited => {
            Redirect::see_other("/login?ratelimited=true").into_response()
        }
        SignupOutcome::Unavailable => Redirect::see_other("/login?system=true").into_response(),
    }
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "auth/check_email.stpl.html")]
struct CheckEmailPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<CheckEmailContent> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "auth/check_email_content.stpl.html")]
struct CheckEmailContent {
    email: String,
}

/// Landing page after a signup was deferred behind an emailed completion
/// link, telling the visitor to go click it.
#[handler]
#[tracing::instrument]
pub async fn check_email_page(
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<Response, anyhow::Error> {
    let email: String = session.get("pending_signup_email").unwrap_or_default();

    let layout = StandardLayout::new("/login", CheckEmailContent { email })
        .prepare(user_info.clone())
        .await?;

    CheckEmailPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "auth/signup_link_expired.stpl.html")]
struct SignupLinkExpiredPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<SignupLinkExpiredContent> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "auth/signup_link_expired_content.stpl.html")]
struct SignupLinkExpiredContent {}

#[derive(Debug, serde::Deserialize)]
struct CompleteSignupParameters {
    code: Option<String>,
}

/// Redeem an emailed signup-completion link: the deferred account is created
/// now, the visitor is logged in, and their original destination restored.
#[handler]
#[tracing::instrument]
pub async fn complete_signup_action(
    session: &Session,
    Authenticated(user_info): Authenticated,
    Query(params): Query<CompleteSignupParameters>,
) -> Result<Response, anyhow::Error> {
    let code = params.code.unwrap_or_default();

    if !code.is_empty() {
        match request::<Option<CompleteSignupResponse>>(
            "user.auth.complete_signup",
            CompleteSignup { code },
        )
        .await
        {
            Ok(Some(completed)) => {
                // Rotate the session ID on this privilege transition to
                // prevent session fixation.
                session.renew();
                session.set("basic_info", &completed.basic_info);

                // Clear signup-specific session data
                session.remove("signup_protected");
                session_tags::clear(session);
                session.remove("auth_next");
                session.remove("pending_signup_email");

                log_activity(
                    None,
                    "user_signup",
                    completed.basic_info.user_id,
                    None,
                    completed.next.clone(),
                )
                .await
                .ok();

                return Ok(
                    Redirect::see_other(completed.next.unwrap_or_else(|| "/".to_string()))
                        .into_response(),
                );
            }
            Ok(None) => {
                // Fall through to the expired-link page
            }
            Err(_) => {
                return Ok(Redirect::see_other("/login?system=true").into_response());
            }
        }
    }

    let layout = StandardLayout::new("/login", SignupLinkExpiredContent {})
        .prepare(user_info.clone())
        .await?;

    let mut response = SignupLinkExpiredPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await?;

    response.set_status(poem::http::StatusCode::GONE);

    Ok(response)
}

#[derive(Debug, serde::Deserialize)]
struct LoginForm {
    email: String,
    password: String,
}

/// How an attempt to sign in with a password came out.
pub enum PasswordLogin {
    /// The session is signed in to this account.
    SignedIn(BasicInfo),
    /// The email address and password don't match an account.
    Refused,
    /// Something on our side failed.
    Unavailable,
}

/// Sign the session in with an email address and password, applying what
/// the session was holding for the account: signup-context tags, a
/// staked-out badge, held training steps. `next` is only recorded.
pub async fn log_in_with_password(
    session: &Session,
    email: String,
    password: String,
    next: Option<String>,
) -> PasswordLogin {
    let login_request = Login { email, password };

    // Call user-service's login via NATS
    match request_with_retry::<Option<BasicInfo>>("user.auth.login", login_request).await {
        Ok(Some(basic_info)) => {
            apply_session_tags(session, basic_info.user_id).await;

            // Store user info in session. Rotate the session ID on this
            // privilege transition to prevent session fixation.
            session.renew();
            session.set("basic_info", &basic_info);

            grant_staked_out_badge(session, basic_info.user_id).await;
            apply_pending_transitions(session, basic_info.user_id).await;

            log_activity(None, "user_login", basic_info.user_id, None, next)
                .await
                .ok();

            PasswordLogin::SignedIn(basic_info)
        }
        Ok(None) => PasswordLogin::Refused,
        // This should never happen since request_with_retry retries indefinitely
        // But we include it for completeness
        Err(_) => PasswordLogin::Unavailable,
    }
}

#[handler]
#[tracing::instrument]
pub async fn login_action(session: &Session, Form(form): Form<LoginForm>) -> Response {
    let next: Option<String> = session
        .get::<String>("auth_next")
        .filter(|n| is_local_next(n));

    match log_in_with_password(session, form.email, form.password, next.clone()).await {
        PasswordLogin::SignedIn(_) => post_auth_redirect(session, next),
        PasswordLogin::Refused => {
            // Login failed - redirect back with error
            if let Some(next) = next {
                Redirect::see_other(format!("/login?password=true&next={next}")).into_response()
            } else {
                Redirect::see_other("/login?password=true").into_response()
            }
        }
        PasswordLogin::Unavailable => {
            if let Some(next) = next {
                Redirect::see_other(format!("/login?system=true&next={next}")).into_response()
            } else {
                Redirect::see_other("/login?system=true").into_response()
            }
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct LogoutQuery {
    next: Option<String>,
    page: Option<String>,
}

#[handler]
#[tracing::instrument]
pub async fn logout_action(
    session: &Session,
    Query(query): Query<LogoutQuery>,
) -> impl IntoResponse {
    let Some(info): Option<BasicInfo> = session.get("basic_info") else {
        return Redirect::see_other("/");
    };

    // Destroy the whole server-side session (not just the identity key) so the
    // session ID cannot be reused and no residual state survives logout.
    session.purge();

    log_activity(None, "logout", info.user_id, None, None)
        .await
        .ok();

    // Someone logging out from the blog's embedded header goes back there;
    // see [pages_return_url]. Otherwise a path here, else home. Both are
    // checked by [is_local_next], so neither can be aimed off-site.
    let redirect_to = query
        .page
        .as_deref()
        .and_then(pages_return_url)
        .unwrap_or_else(|| {
            query
                .next
                .as_deref()
                .filter(|next| is_local_next(next))
                .unwrap_or("/")
                .to_string()
        });

    Redirect::see_other(redirect_to)
}

// WebAuthn Authentication Start - for login flow
#[handler]
#[tracing::instrument]
pub async fn webauthn_authentication_start(
    session: &Session,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let email = req
        .get("email")
        .and_then(|v| v.as_str())
        .map(|s| {
            let trimmed = s.trim();
            // Validate email format if provided (email is optional for usernameless auth)
            if !trimmed.is_empty() {
                if trimmed.len() > 254 {
                    return None; // Invalid email, will be filtered out
                }
                // Basic email validation: must contain @ and have reasonable structure
                if !trimmed.contains('@') || trimmed.starts_with('@') || trimmed.ends_with('@') {
                    return None; // Invalid email, will be filtered out
                }
                // Check for multiple @ symbols
                if trimmed.matches('@').count() != 1 {
                    return None; // Invalid email, will be filtered out
                }
            }
            Some(trimmed.to_string())
        })
        .flatten();

    let webauthn_req = WebAuthnAuthenticationStartRequest { email };

    match request::<WebAuthnAuthenticationStartResponse>(
        "user.webauthn.authentication_start",
        webauthn_req,
    )
    .await
    {
        Ok(response) => {
            // Store authentication state in session for later use
            session.set(
                "webauthn_authentication_state",
                &response.authentication_state_json,
            );
            Json(serde_json::json!({
                "success": true,
                "request_options": response.request_options_json
            }))
            .into_response()
        }
        Err(e) => Json(serde_json::json!({"error": format!("Authentication start failed: {}", e)}))
            .into_response(),
    }
}

// WebAuthn Authentication Complete - for login flow
#[handler]
#[tracing::instrument]
pub async fn webauthn_authentication_complete(
    session: &Session,
    Json(req): Json<serde_json::Value>,
) -> Response {
    // Get authentication state from session
    let authentication_state_json: String = match session.get("webauthn_authentication_state") {
        Some(state) => state,
        None => {
            return Json(serde_json::json!({"error": "No authentication state found"}))
                .into_response();
        }
    };

    let credential_json = match req.get("credential") {
        Some(cred_value) => {
            // Validate that credential is an object with required WebAuthn fields
            if !cred_value.is_object() {
                session.remove("webauthn_authentication_state");
                return Json(serde_json::json!({"error": "Credential must be an object"}))
                    .into_response();
            }

            let cred_obj = cred_value.as_object().unwrap();

            // Check for required WebAuthn credential fields
            let required_fields = ["id", "rawId", "type", "response"];
            for field in required_fields {
                if !cred_obj.contains_key(field) {
                    session.remove("webauthn_authentication_state");
                    return Json(serde_json::json!({"error": format!("Missing required credential field: {}", field)})).into_response();
                }
            }

            // Validate credential type
            if let Some(cred_type) = cred_obj.get("type").and_then(|v| v.as_str()) {
                if cred_type != "public-key" {
                    session.remove("webauthn_authentication_state");
                    return Json(serde_json::json!({"error": "Invalid credential type, must be 'public-key'"})).into_response();
                }
            } else {
                session.remove("webauthn_authentication_state");
                return Json(serde_json::json!({"error": "Credential type must be a string"}))
                    .into_response();
            }

            // Validate response object structure for authentication
            if let Some(response_obj) = cred_obj.get("response") {
                if !response_obj.is_object() {
                    session.remove("webauthn_authentication_state");
                    return Json(
                        serde_json::json!({"error": "Credential response must be an object"}),
                    )
                    .into_response();
                }

                let response_fields = ["clientDataJSON", "authenticatorData", "signature"];
                let resp_obj = response_obj.as_object().unwrap();
                for field in response_fields {
                    if !resp_obj.contains_key(field) {
                        session.remove("webauthn_authentication_state");
                        return Json(serde_json::json!({"error": format!("Missing required response field: {}", field)})).into_response();
                    }
                }
            }

            match serde_json::to_string(cred_value) {
                Ok(json_str) => json_str,
                Err(_) => {
                    session.remove("webauthn_authentication_state");
                    return Json(serde_json::json!({"error": "Failed to serialize credential"}))
                        .into_response();
                }
            }
        }
        None => {
            // Clear authentication state on missing credential
            session.remove("webauthn_authentication_state");
            return Json(serde_json::json!({"error": "Missing credential"})).into_response();
        }
    };

    let webauthn_req = WebAuthnAuthenticationCompleteRequest {
        authentication_state_json,
        credential_json,
    };

    match request::<WebAuthnAuthenticationCompleteResponse>(
        "user.webauthn.authentication_complete",
        webauthn_req,
    )
    .await
    {
        Ok(response) => {
            // Clear authentication state from session
            session.remove("webauthn_authentication_state");

            if let Some(basic_info) = response.basic_info {
                // Store user info in session (same as regular login).
                // Rotate the session ID to prevent session fixation.
                session.renew();
                session.set("basic_info", &basic_info);

                let next: Option<String> = session
                    .get::<String>("auth_next")
                    .filter(|n| is_local_next(n));

                // Clear signup-specific session data if any
                session.remove("signup_protected");

                // The same context the password lane applies. This used to
                // drop the tags on the floor, which meant that whether a
                // partner's cohort tag reached the account depended on which
                // way its owner happened to sign in.
                apply_session_tags(session, basic_info.user_id).await;

                grant_staked_out_badge(session, basic_info.user_id).await;
                apply_pending_transitions(session, basic_info.user_id).await;

                log_activity(
                    None,
                    "webauthn_login",
                    basic_info.user_id,
                    None,
                    next.clone(),
                )
                .await
                .ok();

                // Return success with redirect URL
                Json(serde_json::json!({
                    "success": true,
                    "redirect": next.unwrap_or_else(|| "/".to_string())
                }))
                .into_response()
            } else {
                // Clear authentication state on failure to prevent replay attacks
                session.remove("webauthn_authentication_state");

                Json(serde_json::json!({
                    "error": response.error_message.unwrap_or("Authentication failed".to_string())
                }))
                .into_response()
            }
        }
        Err(e) => {
            // Clear authentication state on error to prevent replay attacks
            session.remove("webauthn_authentication_state");

            Json(serde_json::json!({"error": format!("Authentication complete failed: {}", e)}))
                .into_response()
        }
    }
}

// WebAuthn Registration Start - for signup flow after user creation
#[handler]
#[tracing::instrument]
pub async fn webauthn_registration_start(
    session: &Session,
    Json(req): Json<serde_json::Value>,
) -> Response {
    // Get or generate user_id for registration
    let user_id: Ulid = match session.get("pending_user_id") {
        Some(id) => id, // Existing flow - user already created
        None => {
            // Direct passkey signup - generate new ULID and store for later user creation
            let new_user_id = Ulid::new();
            session.set("pending_user_id", &new_user_id);
            new_user_id
        }
    };

    let username = match req.get("username").and_then(|v| v.as_str()) {
        Some(u) => {
            let trimmed = u.trim();
            // Validate email address format for username
            if trimmed.is_empty() {
                return Json(serde_json::json!({"error": "Email address cannot be empty"}))
                    .into_response();
            }
            if trimmed.len() > 254 {
                return Json(
                    serde_json::json!({"error": "Email address too long (max 254 characters)"}),
                )
                .into_response();
            }
            // Basic email validation: must contain @ and have reasonable structure
            if !trimmed.contains('@') || trimmed.starts_with('@') || trimmed.ends_with('@') {
                return Json(serde_json::json!({"error": "Invalid email address format"}))
                    .into_response();
            }
            // Check for multiple @ symbols
            if trimmed.matches('@').count() != 1 {
                return Json(serde_json::json!({"error": "Invalid email address format"}))
                    .into_response();
            }
            trimmed.to_string()
        }
        None => return Json(serde_json::json!({"error": "Missing email address"})).into_response(),
    };

    let display_name = match req.get("display_name").and_then(|v| v.as_str()) {
        Some(d) => {
            let trimmed = d.trim();
            if trimmed.len() > 128 {
                return Json(
                    serde_json::json!({"error": "Display name too long (max 128 characters)"}),
                )
                .into_response();
            }
            if trimmed.is_empty() {
                generate_random_display_name() // Use random name as fallback if display name is empty
            } else {
                trimmed.to_string()
            }
        }
        None => generate_random_display_name(), // Use random name as fallback for display name
    };

    let webauthn_req = WebAuthnRegistrationStartRequest {
        user_id,
        username,
        display_name,
    };

    match request::<WebAuthnRegistrationStartResponse>(
        "user.webauthn.registration_start",
        webauthn_req,
    )
    .await
    {
        Ok(response) => {
            // Store registration state in session for later use
            session.set(
                "webauthn_registration_state",
                &response.registration_state_json,
            );
            Json(serde_json::json!({
                "success": true,
                "creation_options": response.creation_options_json
            }))
            .into_response()
        }
        Err(e) => Json(serde_json::json!({"error": format!("Registration start failed: {}", e)}))
            .into_response(),
    }
}

// WebAuthn Registration Complete - for signup flow
#[handler]
#[tracing::instrument]
pub async fn webauthn_registration_complete(
    session: &Session,
    RealIp(client_ip): RealIp,
    Json(req): Json<serde_json::Value>,
) -> Response {
    // Get registration state and user_id from session
    let registration_state_json: String = match session.get("webauthn_registration_state") {
        Some(state) => state,
        None => {
            return Json(serde_json::json!({"error": "No registration state found"}))
                .into_response();
        }
    };

    let user_id: Ulid = match session.get("pending_user_id") {
        Some(id) => id,
        None => {
            return Json(serde_json::json!({"error": "No pending user registration"}))
                .into_response();
        }
    };

    let credential_json = match req.get("credential") {
        Some(cred_value) => {
            // Validate that credential is an object with required WebAuthn fields
            if !cred_value.is_object() {
                session.remove("webauthn_registration_state");
                session.remove("pending_user_id");
                return Json(serde_json::json!({"error": "Credential must be an object"}))
                    .into_response();
            }

            let cred_obj = cred_value.as_object().unwrap();

            // Check for required WebAuthn credential fields
            let required_fields = ["id", "rawId", "type", "response"];
            for field in required_fields {
                if !cred_obj.contains_key(field) {
                    session.remove("webauthn_registration_state");
                    session.remove("pending_user_id");
                    return Json(serde_json::json!({"error": format!("Missing required credential field: {}", field)})).into_response();
                }
            }

            // Validate credential type
            if let Some(cred_type) = cred_obj.get("type").and_then(|v| v.as_str()) {
                if cred_type != "public-key" {
                    session.remove("webauthn_registration_state");
                    session.remove("pending_user_id");
                    return Json(serde_json::json!({"error": "Invalid credential type, must be 'public-key'"})).into_response();
                }
            } else {
                session.remove("webauthn_registration_state");
                session.remove("pending_user_id");
                return Json(serde_json::json!({"error": "Credential type must be a string"}))
                    .into_response();
            }

            // Validate response object structure
            if let Some(response_obj) = cred_obj.get("response") {
                if !response_obj.is_object() {
                    session.remove("webauthn_registration_state");
                    session.remove("pending_user_id");
                    return Json(
                        serde_json::json!({"error": "Credential response must be an object"}),
                    )
                    .into_response();
                }

                let response_fields = ["clientDataJSON", "attestationObject"];
                let resp_obj = response_obj.as_object().unwrap();
                for field in response_fields {
                    if !resp_obj.contains_key(field) {
                        session.remove("webauthn_registration_state");
                        session.remove("pending_user_id");
                        return Json(serde_json::json!({"error": format!("Missing required response field: {}", field)})).into_response();
                    }
                }
            }

            match serde_json::to_string(cred_value) {
                Ok(json_str) => json_str,
                Err(_) => {
                    session.remove("webauthn_registration_state");
                    session.remove("pending_user_id");
                    return Json(serde_json::json!({"error": "Failed to serialize credential"}))
                        .into_response();
                }
            }
        }
        None => {
            // Clear registration state on missing credential
            session.remove("webauthn_registration_state");
            session.remove("pending_user_id");
            return Json(serde_json::json!({"error": "Missing credential"})).into_response();
        }
    };

    // Check if this request includes signup data (direct passkey signup)
    let signup_data = req.get("signup_data");

    let webauthn_req = WebAuthnRegistrationCompleteRequest {
        user_id,
        registration_state_json,
        credential_json,
    };

    match request::<WebAuthnRegistrationCompleteResponse>(
        "user.webauthn.registration_complete",
        webauthn_req,
    )
    .await
    {
        Ok(response) => {
            // Clear registration state from session
            session.remove("webauthn_registration_state");

            if response.success {
                // Check if this is a direct passkey signup with data in request
                if let Some(signup_data) = signup_data {
                    // Direct passkey signup - create user now
                    let handle = signup_data
                        .get("handle")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let email = signup_data
                        .get("email")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let zipcode = signup_data
                        .get("zipcode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let newsletter = signup_data
                        .get("newsletter")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    // Get session values for protected and tags (if any)
                    let protected = session.get::<bool>("signup_protected").unwrap_or(false);
                    let tags = session_tags::list(session);

                    // Get user location from location service
                    let point: Option<String> = if let Some(addr) = client_ip {
                        match request::<LocationInfo>("location.ip", addr.to_string()).await {
                            Ok(location) => location.as_point().map(|point| point.to_string()),
                            Err(_) => None,
                        }
                    } else {
                        None
                    };
                    // Create the user account now that passkey registration succeeded
                    let signup_request = Signup {
                        handle: handle.clone(),
                        email: email.clone(),
                        password: String::new(), // Empty password for passkey users
                        newsletter: newsletter,
                        zipcode: zipcode,
                        protected: protected,
                        point: point,
                        tags: tags,
                        disabled_password: true, // Mark as disabled password account
                        badgeclass: staked_out_badge(session),
                        attribution: first_touch(session),
                        pending_transitions: pending_transitions::list(session),
                        join_project: None,
                    };

                    match request_with_retry::<Option<BasicInfo>>(
                        "user.auth.signup",
                        signup_request,
                    )
                    .await
                    {
                        Ok(Some(basic_info)) => {
                            // Store user info in session (complete the login).
                            // Rotate the session ID to prevent session fixation.
                            session.renew();
                            session.set("basic_info", &basic_info);

                            // Clear pending user data
                            session.remove("pending_user_id");

                            let next: Option<String> = session
                                .get::<String>("auth_next")
                                .filter(|n| is_local_next(n));

                            // Clear signup-specific session data
                            session.remove("signup_protected");
                            session_tags::clear(session);
                            clear_staked_out_badge(session);
                            pending_transitions::clear(session);

                            log_activity(
                                None,
                                "passkey_signup_complete",
                                basic_info.user_id,
                                None,
                                next.clone(),
                            )
                            .await
                            .ok();

                            Json(serde_json::json!({
                                "success": true,
                                "credential_id": response.credential_id,
                                "logged_in": true,
                                "redirect": next.unwrap_or_else(|| "/".to_string())
                            }))
                            .into_response()
                        }
                        Ok(None) => {
                            // Clear session state on user creation failure
                            session.remove("pending_user_id");

                            Json(serde_json::json!({
                                "error": crate::i18n::tr("That email address already belongs to a SciStarter account. Log in below to continue.")
                            }))
                            .into_response()
                        }
                        Err(e) => {
                            // Clear session state on user creation error
                            session.remove("pending_user_id");

                            Json(serde_json::json!({
                                "error": format!("Failed to create user account: {}", e)
                            }))
                            .into_response()
                        }
                    }
                } else {
                    // This is an existing user adding a passkey (not implemented yet)
                    Json(serde_json::json!({
                        "success": true,
                        "credential_id": response.credential_id,
                        "logged_in": false
                    }))
                    .into_response()
                }
            } else {
                // Clear session state on registration failure to prevent replay attacks
                session.remove("pending_user_id");

                Json(serde_json::json!({
                    "error": response.error_message.unwrap_or("Registration failed".to_string())
                }))
                .into_response()
            }
        }
        Err(e) => {
            // Clear session state on error to prevent replay attacks
            session.remove("webauthn_registration_state");
            session.remove("pending_user_id");

            Json(serde_json::json!({"error": format!("Registration complete failed: {}", e)}))
                .into_response()
        }
    }
}

// WebAuthn API routes with rate limiting
/// Rate limiter for password credential endpoints (login and signup) to throttle
/// brute-force and credential-stuffing attempts. Keyed per client IP, consistent
/// with the WebAuthn limiter. Returns a fresh instance per call so it can be
/// applied to more than one route.
pub fn credential_rate_limit() -> RateLimitMiddleware {
    let config = RateLimitConfig::new(10, Duration::from_secs(300))
        .with_error_message("Too many attempts. Please try again in a few minutes.");
    RateLimitMiddleware::new(config)
}

pub fn webauthn_routes() -> impl Endpoint {
    // Conservative rate limits for WebAuthn: 5 attempts per 5 minutes per IP
    let rate_limit_config = RateLimitConfig::new(5, Duration::from_secs(300))
        .with_error_message("Too many WebAuthn attempts. Please try again in 5 minutes.");

    Route::new()
        .at("/auth-start", post(webauthn_authentication_start))
        .at("/auth-complete", post(webauthn_authentication_complete))
        .at("/reg-start", post(webauthn_registration_start))
        .at("/reg-complete", post(webauthn_registration_complete))
        .with(RateLimitMiddleware::new(rate_limit_config))
}

#[cfg(test)]
mod tests {
    use super::{pages_return_url, signup_will_be_protected};
    use crate::session::session_tags;
    use poem::session::Session;

    /// The flag a protected signup context sets directly.
    #[test]
    fn the_session_flag_protects_the_signup() {
        let session = Session::default();
        session.set("signup_protected", true);

        assert!(signup_will_be_protected(&session));
    }

    /// The other half: a tag that names a cohort needing protection, which
    /// user-service turns into a protected account whether or not the flag
    /// was ever set.
    #[test]
    fn a_protecting_tag_protects_the_signup() {
        let session = Session::default();
        session_tags::add(&session, "girlscout");

        assert!(signup_will_be_protected(&session));
    }

    /// A `/go/` link carries its tag under `signup_tags` rather than through
    /// `add`, and that spelling has to reach this decision too.
    #[test]
    fn a_go_link_tag_protects_the_signup() {
        let session = Session::default();
        session.set("signup_tags", "girlscout");

        assert!(signup_will_be_protected(&session));
    }

    /// Written however the partner typed it: the form would otherwise offer
    /// an opt-in that the created account discards.
    #[test]
    fn the_tag_is_matched_however_it_is_written() {
        let session = Session::default();
        session_tags::add(&session, " #GirlScout ");

        assert!(signup_will_be_protected(&session));
    }

    #[test]
    fn an_ordinary_signup_is_not_protected() {
        let session = Session::default();
        session_tags::add(&session, "some-partner");

        assert!(!signup_will_be_protected(&session));
    }

    #[test]
    fn a_signup_with_no_context_at_all_is_not_protected() {
        assert!(!signup_will_be_protected(&Session::default()));
    }

    #[test]
    fn a_page_return_is_built_from_a_path_only() {
        assert_eq!(
            pages_return_url("/calendar/").as_deref(),
            Some("https://pages.scistarter.org/calendar/")
        );

        // Nothing that could extend the host past pages.scistarter.org.
        for bad in [
            "//evil.example",
            ".evil.example/",
            "@evil.example/",
            "https://evil.example/",
            "",
        ] {
            assert!(pages_return_url(bad).is_none(), "{bad:?} should be refused");
        }
    }
}
