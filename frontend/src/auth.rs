use common::{
    message::{
        location::LocationInfo,
        user::{BasicInfo, Login, Signup},
        webauthn::{
            WebAuthnAuthenticationCompleteRequest, WebAuthnAuthenticationCompleteResponse,
            WebAuthnAuthenticationStartRequest, WebAuthnAuthenticationStartResponse,
            WebAuthnRegistrationCompleteRequest, WebAuthnRegistrationCompleteResponse,
            WebAuthnRegistrationStartRequest, WebAuthnRegistrationStartResponse,
        },
    },
    service::{log_activity, request, request_with_retry},
};
use poem::{
    Endpoint, EndpointExt, IntoResponse, Response, Route, handler, post,
    session::Session,
    web::{Form, Json, Query, RealIp, Redirect},
};
use rand::{Rng, thread_rng};
use std::sync::Arc;
use std::time::Duration;
use ulid::Ulid;

use crate::{
    component::{Component, Head, StandardLayout},
    middleware::{
        authorization::Authenticated,
        rate_limit::{RateLimitConfig, RateLimitMiddleware},
    },
    session::session_tags,
};

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
    unauthorized: Option<bool>,
    next: Option<String>,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "auth/login_signup.stpl.html")]
struct LoginSignupPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<LoginSignupContent> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "auth/login_signup_content.stpl.html")]
struct LoginSignupContent {
    system: bool,
    password: bool,
    unauthorized: bool,
    #[allow(dead_code)]
    user_info: Option<Arc<BasicInfo>>,
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
        if next.starts_with("/") {
            session.set("auth_next", next);
        }
    }

    let layout = StandardLayout::new(
        "/login",
        LoginSignupContent {
            system: params.system.unwrap_or_default(),
            password: params.password.unwrap_or_default(),
            unauthorized: params.unauthorized.unwrap_or_default(),
            user_info: user_info.clone(),
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
}

#[handler]
#[tracing::instrument]
pub async fn signup_action(
    session: &Session,
    RealIp(client_ip): RealIp,
    Form(form): Form<SignupForm>,
) -> Response {
    let next: Option<String> = session.get("auth_next");

    let use_passkey = form.use_passkey.unwrap_or(false);

    // Validate password confirmation (only if not using passkey)
    if !use_passkey && form.password1 != form.password2 {
        return Redirect::see_other("/login?password=mismatch").into_response();
    }

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

    // For passkey users, this shouldn't be reached since JavaScript handles the flow
    // This is only for password users now
    if !use_passkey {
        // Regular password signup - create user immediately
        let signup_request = Signup {
            handle: form.handle,
            email: form.email,
            password: form.password1,
            newsletter: form.newsletter.unwrap_or(false),
            zipcode: form.zipcode,
            protected,
            point,
            tags,
            disabled_password: false, // Regular password account
        };

        // Call user-service's signup via NATS
        match request_with_retry::<Option<BasicInfo>>("user.auth.signup", signup_request).await {
            Ok(Some(basic_info)) => {
                // Regular password signup - log them in immediately
                session.set("basic_info", &basic_info);

                // Clear signup-specific session data
                session.remove("signup_protected");
                session_tags::clear(session);

                log_activity(None, "user_signup", basic_info.user_id, None, next.clone())
                    .await
                    .ok();

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
                // This should never happen since request_with_retry retries indefinitely
                // But we include it for completeness
                Redirect::see_other("/login?signup=error").into_response()
            }
        }
    } else {
        // Passkey signup - this shouldn't be reached since JavaScript handles it
        // Return an error if somehow we get here
        Redirect::see_other("/login?signup=error").into_response()
    }
}

#[derive(Debug, serde::Deserialize)]
struct LoginForm {
    email: String,
    password: String,
}

#[handler]
#[tracing::instrument]
pub async fn login_action(session: &Session, Form(form): Form<LoginForm>) -> Response {
    let next: Option<String> = session.get("auth_next");

    let login_request = Login {
        email: form.email.clone(),
        password: form.password,
    };

    // Call user-service's login via NATS
    match request_with_retry::<Option<BasicInfo>>("user.auth.login", login_request).await {
        Ok(Some(basic_info)) => {
            // Merge session tags with user tags if any exist
            let session_tags = session_tags::clear(session);
            if !session_tags.is_empty() {
                // Add session tags to the user's account
                for tag in session_tags {
                    let _ = common::service::send(
                        "user.tags.add",
                        (basic_info.user_id.to_string(), tag),
                    )
                    .await;
                }
            }

            // Store user info in session
            session.set("basic_info", &basic_info);

            log_activity(None, "user_login", basic_info.user_id, None, next.clone())
                .await
                .ok();

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
            if let Some(next) = next {
                Redirect::see_other(format!("/login?password=true&next={next}")).into_response()
            } else {
                Redirect::see_other("/login?password=true").into_response()
            }
        }
        Err(_) => {
            // This should never happen since request_with_retry retries indefinitely
            // But we include it for completeness
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

    session.remove("basic_info");

    log_activity(None, "logout", info.user_id, None, None)
        .await
        .ok();

    // Validate next parameter - must start with / to prevent open redirect
    let redirect_to = query
        .next
        .as_deref()
        .filter(|next| next.starts_with('/'))
        .unwrap_or("/");

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
                // Store user info in session (same as regular login)
                session.set("basic_info", &basic_info);

                let next: Option<String> = session.get("auth_next");

                // Clear signup-specific session data if any
                session.remove("signup_protected");
                session_tags::clear(session);

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
                    };

                    match request_with_retry::<Option<BasicInfo>>(
                        "user.auth.signup",
                        signup_request,
                    )
                    .await
                    {
                        Ok(Some(basic_info)) => {
                            // Store user info in session (complete the login)
                            session.set("basic_info", &basic_info);

                            // Clear pending user data
                            session.remove("pending_user_id");

                            let next: Option<String> = session.get("auth_next");

                            // Clear signup-specific session data
                            session.remove("signup_protected");
                            session_tags::clear(session);

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
                                "error": "Failed to create user account after passkey registration"
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
