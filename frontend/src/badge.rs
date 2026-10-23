use std::sync::Arc;

use anyhow::{Error, anyhow};
use common::badge::{Alignment, Badge};
use common::message::user::{
    BasicInfo, UserAndBadge, UserBadge, UserEmailInfo, UserEmailInfoRequest,
};
use jiff::Timestamp;
use poem::{
    IntoResponse, Response, Route, handler,
    http::{StatusCode, header},
    session::Session,
    web::{Html, Json, Path, Query},
};
use sailfish::TemplateOnce;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

mod card;
mod text;

use crate::i18n::{Lang, current_lang, tr, tr_in};
use card::CardText;
use text::{BadgeText, LanguageLink, badge_artwork, image_query};

const DOMAIN: &str = env!("DOMAIN");

/// The language of a request for the badge *image*.
///
/// Only an explicit `?lang=` is honoured, never the session or the browser,
/// because the image URL recorded in the assertion carries no query string:
/// anything following the credential gets the badge's own artwork however
/// its reader happens to be browsing. A translated page asks for translated
/// artwork by saying so in the URL -- see [`text::image_query`].
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ImageParams {
    lang: Option<String>,
}

impl ImageParams {
    fn lang(&self) -> Lang {
        self.lang
            .as_deref()
            .and_then(Lang::from_tag)
            .unwrap_or_default()
    }
}

/// What a request for one of the badge URLs is answered with.
///
/// A badge's URL and each earner's are at once the `id` of an Open Badges
/// object, which a verifier fetches for the credential, and the page people
/// are sent to. Open Badges 2.0 ("Setting Content-Type") makes the object
/// the default and lets a client that asks for something else have it, so a
/// request naming no type gets the object, and so does anything naming a
/// JSON type. Matching on `json` rather than one full type means a validator
/// asking only for `application/ld+json`, the type a badge object actually
/// is, is answered with the object; no browser sends `json` in its Accept
/// header. A browser does send `*/*`, as do the crawlers that build link
/// previews, and those get the page. scistarter.org answers the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Representation {
    /// The Open Badges object, as this JSON media type.
    Json(JsonType),
    /// The badge artwork.
    Png,
    /// The web page about the badge.
    Html,
}

impl Representation {
    fn negotiate(accept: Option<&str>) -> Self {
        let Some(accept) = accept.filter(|accept| !accept.trim().is_empty()) else {
            return Self::Json(JsonType::LdJson);
        };

        if accept.contains("json") {
            Self::Json(JsonType::for_accept(Some(accept)))
        } else if accept.contains("image/png") || accept.contains("image/*") {
            Self::Png
        } else {
            Self::Html
        }
    }
}

/// The media type a badge object's JSON is served as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsonType {
    /// `application/ld+json`, which is what an Open Badges object is.
    LdJson,
    /// `application/json`, for a client that names only that.
    Json,
}

impl JsonType {
    /// JSON-LD by default, and plain JSON when that is the only JSON type
    /// the request names, as Open Badges 2.0 asks of an issuer.
    fn for_accept(accept: Option<&str>) -> Self {
        match accept {
            Some(accept)
                if accept.contains("application/json")
                    && !accept.contains("application/ld+json") =>
            {
                Self::Json
            }
            _ => Self::LdJson,
        }
    }

    fn content_type(self) -> &'static str {
        match self {
            Self::LdJson => "application/ld+json",
            Self::Json => "application/json",
        }
    }
}

/// The request's Accept header, if it has a readable one.
fn accept(req: &poem::Request) -> Option<&str> {
    req.headers()
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
}

/// A badge object as JSON, labelled with the type `json_type` names. The
/// body is the same whatever was asked for; only its label varies.
fn json_response(json_type: JsonType, object: &impl Serialize) -> poem::Result<Response> {
    let body = serde_json::to_string(object).map_err(|e| {
        tracing::error!("Failed to serialize badge object: {}", e);
        poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, json_type.content_type())
        .header(header::VARY, "Accept")
        .body(body))
}

pub fn routes() -> Route {
    Route::new()
        .at("/issuer.json", poem::get(issuer_json))
        .at("/:badgeclass", poem::get(badge_class))
        .at("/:badgeclass/badge.png", poem::get(badgeclass_png))
        .at("/:badgeclass/card.png", poem::get(badgeclass_card))
        .at("/:badgeclass/:user_id", poem::get(badge_instance))
        .at(
            "/:badgeclass/:user_id/assertion.json",
            poem::get(assertion_json),
        )
        .at("/:badgeclass/:user_id/badge.png", poem::get(badge_png))
        .at("/:badgeclass/:user_id/oembed.json", poem::get(oembed_json))
        .at("/:badgeclass/:user_id/oembed.xml", poem::get(oembed_xml))
}

#[derive(Serialize)]
struct IssuerJson {
    #[serde(rename = "@context")]
    context: String,
    #[serde(rename = "type")]
    type_: String,
    id: String,
    name: String,
    url: String,
    email: String,
    description: String,
    image: String,
}

fn make_issuer_json() -> IssuerJson {
    IssuerJson {
        context: "https://w3id.org/openbadges/v2".to_string(),
        type_: "Issuer".to_string(),
        id: format!("https://{}/badges/issuer.json", DOMAIN),
        name: "SciStarter".to_string(),
        url: format!("https://{}/", DOMAIN),
        email: "info@scistarter.org".to_string(),
        description: "SciStarter makes it easy for anyone to find and participate in authentic online and local science through citizen science, community science, and crowdsourced research projects.".to_string(),
        image: format!("https://{}/-static/img/logo--white_tag-rgb.svg", DOMAIN),
    }
}

#[handler]
async fn issuer_json(req: &poem::Request) -> poem::Result<Response> {
    json_response(JsonType::for_accept(accept(req)), &make_issuer_json())
}

#[derive(Serialize)]
struct BadgeClassJson {
    #[serde(rename = "@context")]
    context: String,
    #[serde(rename = "type")]
    type_: String,
    id: String,
    name: String,
    description: String,
    image: String,
    criteria: BadgeCriteria,
    issuer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
    /// The skills the badge is aligned to. The assertion has carried these
    /// all along; the hosted BadgeClass used to leave them out, so the two
    /// descriptions of the same badge disagreed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    alignment: Vec<AlignmentJson>,
}

#[derive(Serialize)]
struct BadgeCriteria {
    #[serde(rename = "type")]
    type_: String,
    id: String,
    narrative: String,
}

#[handler]
async fn badge_class(
    req: &poem::Request,
    Path(badgeclass): Path<String>,
) -> poem::Result<Response> {
    let badge = Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| poem::Error::from_status(StatusCode::NOT_FOUND))?;

    let json_type = match Representation::negotiate(accept(req)) {
        Representation::Json(json_type) => json_type,
        // The badge's own artwork, as the BadgeClass names it: canonical,
        // whatever language the reader's session is in.
        Representation::Png => {
            let badge_file = crate::Static::get(badge.template_path())
                .ok_or_else(|| anyhow!("Badge template not found: {}", badge.template_path()))?;

            return Ok(Response::builder()
                .header(header::CONTENT_TYPE, "image/png")
                .header(header::VARY, "Accept")
                .body(badge_file.data.into_owned()));
        }
        Representation::Html => {
            let html_content = render_badgeclass_html(&badge, &badgeclass)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to render badge HTML: {}", e);
                    poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
                })?;
            return Ok(Html(html_content)
                .into_response()
                .with_header("Vary", "Accept")
                .into_response());
        }
    };

    json_response(
        json_type,
        &BadgeClassJson {
            context: "https://w3id.org/openbadges/v2".to_string(),
            type_: "BadgeClass".to_string(),
            id: format!("https://{}/badges/{}", DOMAIN, badgeclass),
            name: badge.name().to_string(),
            description: badge.description().to_string(),
            image: format!("https://{}/badges/{}/badge.png", DOMAIN, badgeclass),
            criteria: BadgeCriteria {
                type_: "Criteria".to_string(),
                // Where the criteria are actually described: the training that
                // earns the badge. It used to name this page, which describes
                // the badge rather than how to earn it.
                id: badge.criteria_url().to_string(),
                // The badge's own text, as it is baked into the image and as
                // scistarter.org serves it. Rendering it to HTML here made the
                // hosted BadgeClass and the baked one disagree.
                narrative: badge.criteria_text().to_string(),
            },
            issuer: format!("https://{}/badges/issuer.json", DOMAIN),
            tags: Some(badge.tags().iter().map(|s| s.to_string()).collect()),
            alignment: alignment_json(&badge),
        },
    )
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct BadgeParams {
    confirm: Option<String>,
    download: Option<bool>,
    /// Chooses the artwork when this URL is fetched as an image rather than
    /// read as a page. The page itself takes its language from the request,
    /// like every other page on the site.
    lang: Option<String>,
}

impl BadgeParams {
    fn image_lang(&self) -> Lang {
        self.lang
            .as_deref()
            .and_then(Lang::from_tag)
            .unwrap_or_default()
    }
}

/// An earner's badge, with what issuing it takes, once it may be handed out.
struct Issued {
    badge: Badge,
    user_ulid: Ulid,
    user_badge: UserBadge,
    user_info: UserEmailInfo,
}

fn is_own_badge(session: &Session, user_ulid: Ulid) -> bool {
    session
        .get::<BasicInfo>("basic_info")
        .is_some_and(|u| u.user_id == user_ulid)
}

/// Whether an earner's badges may be handed out as credentials: their
/// account is verified, or staff have allowed it, which orrery records as
/// the `badge-allowed` flag.
fn released(user_info: &UserEmailInfo) -> bool {
    user_info.verified || user_info.badge_allowed
}

/// The address of the credential `user_id` earned in `badge`: its badge
/// page, and what LinkedIn's copy of it points back at.
fn badge_url(badge: Badge, user_id: &str) -> String {
    format!(
        "https://{}/badges/{}/{}",
        DOMAIN,
        badge.badgeclass(),
        user_id
    )
}

// The earner's own view of the badge page has a section on showing the
// badge off, with one part for LinkedIn's Licenses & Certifications and one
// for posting it. Its phrases are named here, rather than written into the
// template, so the seed test can find them; migration
// `seed_badge_share_text_translations` translates them under exactly this
// wording.
const SHOWCASE_HEADING: &str = "Showcase Your Achievement";
const SHOWCASE_INTRO: &str =
    "Add your verified badge to your professional profile and share it with your network.";
const LINKEDIN_HEADING: &str = "Add to LinkedIn Profile";
const LINKEDIN_EXPLANATION: &str = "Permanently showcase your achievement in the Licenses & Certifications section of your LinkedIn profile, where recruiters and connections can see and verify your credential.";
const SOCIAL_HEADING: &str = "Share on Social";
const SOCIAL_EXPLANATION: &str = "Celebrate your achievement with your network! Share your badge to highlight your citizen science skills and inspire others to get involved.";

/// The button to the training that earns a badge, on its class page and on
/// everyone else's view of an earner's. Migration
/// `seed_badge_share_text_translations` gives it the translations the
/// lowercase wording it replaced had.
const LEARN_HOW_TO_EARN: &str = "Learn How to Earn This Badge";

// Everyone else's view of the badge page has a section for confirming whom
// the badge was issued to. Its phrases are named here for the seed test, as
// the earner's are above. `CONFIRM_INTRO` uses `{name}`.
const CONFIRM_HEADING: &str = "Confirm This Badge's Recipient";
const CONFIRM_AUDIENCE: &str = "For employers, schools, and credential reviewers";
const CONFIRM_INTRO: &str = "Want to confirm that this {name} badge, issued by SciStarter, belongs to a particular person? Enter their email address below to confirm whether it was issued to them.";
const CONFIRM_BUTTON: &str = "Confirm Recipient";

/// The hint shown beneath a LinkedIn "Add to Profile" link, for the app
/// that leaves the issuer blank. The badge page and the earned-badge slot
/// both show it, and migration `seed_badge_linkedin_hint_translation`
/// translates it under exactly this wording.
pub const LINKEDIN_HINT: &str = "On a phone, LinkedIn's app may leave <b>Issuing organization</b> blank. If it does, type <b>SciStarter</b> into that field and pick us from the list.";

/// LinkedIn's "Add to Profile" link for the credential `user_id` earned in
/// `badge` on `issued`, filling in a license or certification entry that
/// points back at [`badge_url`].
///
/// The name is the badge's canonical one, never a translation: it becomes
/// the name of the credential on the earner's profile, which a verifier
/// compares against the baked image.
fn linkedin_add_url(badge: Badge, user_id: &str, issued: &jiff::Zoned) -> String {
    format!(
        "https://www.linkedin.com/profile/add?startTask=CERTIFICATION_NAME&name={}&organizationId=3205961&issueYear={}&issueMonth={}&certUrl={}&certId={}",
        urlencoding::encode(badge.name()),
        issued.year(),
        issued.month(),
        urlencoding::encode(&badge_url(badge, user_id)),
        urlencoding::encode(&format!("{}/{}", badge.badgeclass(), user_id)),
    )
}

/// When `user_badge` was issued, in UTC.
fn issued_on(user_badge: &UserBadge) -> Result<jiff::Zoned, Error> {
    let issued = user_badge
        .when
        .to_string()
        .parse::<Timestamp>()
        .map_err(|e| anyhow!("Failed to parse timestamp: {}", e))?;

    Ok(issued.to_zoned(jiff::tz::TimeZone::UTC))
}

/// What a training page says in its closing step once `user` holds
/// `badge`, with their own links to it; empty until then, so the page claims
/// nothing it cannot back up.
///
/// A badge that is not yet [`released`] is already on the earner's profile,
/// but its badge page -- and LinkedIn's copy of it -- would show nothing
/// yet, so those links wait, and the earner is told what they are waiting
/// for.
pub async fn earned_html(user: &Option<Arc<BasicInfo>>, badge: Badge) -> String {
    let Some(user) = user else {
        return String::new();
    };

    let request = UserAndBadge {
        user_id: user.user_id,
        badge,
    };

    let user_badge: Option<UserBadge> =
        match common::service::request("user.badges.get", &request).await {
            Ok(user_badge) => user_badge,
            Err(e) => {
                tracing::error!("Failed to get badge from user-service: {}", e);
                return String::new();
            }
        };

    let Some(user_badge) = user_badge else {
        return String::new();
    };

    let email_request = UserEmailInfoRequest {
        user_id: user.user_id,
    };

    let links =
        match common::service::request::<UserEmailInfo>("user.info.email_info", &email_request)
            .await
        {
            Ok(user_info) if released(&user_info) => {
                let user_id = user.user_id.to_string();

                match issued_on(&user_badge) {
                    Ok(issued) => Some(EarnedLinks {
                        badge_url: badge_url(badge, &user_id),
                        linkedin_url: linkedin_add_url(badge, &user_id, &issued),
                    }),
                    Err(e) => {
                        tracing::error!("Failed to read when a badge was issued: {}", e);
                        None
                    }
                }
            }
            Ok(_) => None,
            Err(e) => {
                tracing::error!("Failed to get user email: {}", e);
                None
            }
        };

    EarnedSlot {
        name: BadgeText::shown(badge, current_lang()).name,
        links,
    }
    .render_once()
    .unwrap_or_default()
}

struct EarnedLinks {
    badge_url: String,
    linkedin_url: String,
}

#[derive(TemplateOnce)]
#[template(path = "badge/earned_slot.stpl.html")]
struct EarnedSlot {
    name: String,
    links: Option<EarnedLinks>,
}

/// Look up `user_id`'s `badgeclass` badge, provided it may be handed out.
///
/// A badge is earned whether or not the earner's account is verified: the
/// grant is recorded, and the badge is listed on their profile. What waits
/// for verification (or for staff to allow it, which orrery records as the
/// `badge-allowed` flag) is the credential itself: the badge page, the baked
/// image and the assertion. Every handler that serves one of those comes
/// through here, so none of them can hand out what the others hold back.
async fn issued_badge(session: &Session, badgeclass: &str, user_id: &str) -> poem::Result<Issued> {
    let badge = Badge::from_badgeclass(badgeclass)
        .ok_or_else(|| poem::Error::from_status(StatusCode::NOT_FOUND))?;

    let user_ulid = user_id
        .parse::<Ulid>()
        .map_err(|_| poem::Error::from_status(StatusCode::BAD_REQUEST))?;

    let request = UserAndBadge {
        user_id: user_ulid,
        badge,
    };

    let user_badge: Option<UserBadge> = common::service::request("user.badges.get", &request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get badge from user-service: {}", e);
            poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    let user_badge = user_badge.ok_or_else(|| poem::Error::from_status(StatusCode::NOT_FOUND))?;

    let email_request = UserEmailInfoRequest { user_id: user_ulid };

    let user_info: UserEmailInfo = common::service::request("user.info.email_info", &email_request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get user email: {}", e);
            poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    if !released(&user_info) {
        // Only the earner is told why. Anyone else sees the same bare 404 as
        // for a badge that was never earned, so a withheld badge reveals
        // nothing about the account behind it.
        return Err(if is_own_badge(session, user_ulid) {
            poem::Error::from_string(
                tr(
                    "This badge will not be valid until your account is verified. Check your email (including the spam folder) or visit scistarter.org to get your account verified.",
                ),
                StatusCode::NOT_FOUND,
            )
        } else {
            poem::Error::from_status(StatusCode::NOT_FOUND)
        });
    }

    Ok(Issued {
        badge,
        user_ulid,
        user_badge,
        user_info,
    })
}

#[handler]
async fn badge_instance(
    session: &Session,
    req: &poem::Request,
    Path((badgeclass, user_id)): Path<(String, String)>,
    Query(params): Query<BadgeParams>,
) -> poem::Result<Response> {
    let Issued {
        badge,
        user_ulid,
        user_badge,
        user_info,
    } = issued_badge(session, &badgeclass, &user_id).await?;

    let own_badge = is_own_badge(session, user_ulid);

    match Representation::negotiate(accept(req)) {
        Representation::Json(json_type) => {
            let assertion = build_badge_assertion(
                &badgeclass,
                &user_id,
                &badge,
                &user_info.primary_email,
                &user_badge.when.to_string(),
            )
            .map_err(|e| {
                tracing::error!("Failed to build badge assertion: {}", e);
                poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
            })?;
            return json_response(json_type, &assertion);
        }
        Representation::Png => {
            let png_data = bake_badge(
                &badgeclass,
                &user_id,
                &badge,
                &user_badge,
                &user_info.primary_email,
                params.image_lang(),
            )
            .map_err(|e| {
                tracing::error!("Failed to bake badge: {}", e);
                poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
            })?;
            return Ok(Response::builder()
                .header(header::CONTENT_TYPE, "image/png")
                .header(header::VARY, "Accept")
                .body(png_data));
        }
        Representation::Html => {}
    }

    let html_content = render_badge_html(
        &badge,
        &user_badge,
        &user_info.primary_email,
        &badgeclass,
        &user_id,
        &params,
        own_badge,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to render badge HTML: {}", e);
        poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
    })?;
    Ok(Html(html_content)
        .into_response()
        .with_header("Vary", "Accept")
        .into_response())
}

#[derive(Serialize)]
struct BadgeAssertion {
    #[serde(rename = "@context")]
    context: String,
    #[serde(rename = "type")]
    type_: String,
    id: String,
    recipient: BadgeRecipient,
    badge: BadgeClassEmbedded,
    #[serde(rename = "issuedOn")]
    issued_on: String,
    image: String,
    verification: BadgeVerification,
}

#[derive(Serialize)]
struct BadgeRecipient {
    #[serde(rename = "type")]
    type_: String,
    hashed: bool,
    identity: String,
}

#[derive(Serialize)]
struct BadgeVerification {
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Serialize)]
struct BadgeClassEmbedded {
    #[serde(rename = "@context")]
    context: String,
    #[serde(rename = "type")]
    type_: String,
    id: String,
    name: String,
    description: String,
    image: String,
    criteria: BadgeCriteria,
    issuer: IssuerJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    alignment: Vec<AlignmentJson>,
}

#[derive(Serialize)]
struct AlignmentJson {
    #[serde(rename = "targetName")]
    target_name: String,
    #[serde(rename = "targetUrl")]
    target_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "targetDescription")]
    target_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "targetFramework")]
    target_framework: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "targetCode")]
    target_code: Option<String>,
}

#[handler]
async fn assertion_json(
    session: &Session,
    req: &poem::Request,
    Path((badgeclass, user_id)): Path<(String, String)>,
) -> poem::Result<Response> {
    let Issued {
        badge,
        user_badge,
        user_info,
        ..
    } = issued_badge(session, &badgeclass, &user_id).await?;

    let assertion = build_badge_assertion(
        &badgeclass,
        &user_id,
        &badge,
        &user_info.primary_email,
        &user_badge.when.to_string(),
    )
    .map_err(|e| {
        tracing::error!("Failed to build badge assertion: {}", e);
        poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    json_response(JsonType::for_accept(accept(req)), &assertion)
}

#[handler]
async fn badge_png(
    session: &Session,
    Path((badgeclass, user_id)): Path<(String, String)>,
    Query(params): Query<ImageParams>,
) -> poem::Result<Response> {
    let Issued {
        badge,
        user_badge,
        user_info,
        ..
    } = issued_badge(session, &badgeclass, &user_id).await?;

    let png_data = bake_badge(
        &badgeclass,
        &user_id,
        &badge,
        &user_badge,
        &user_info.primary_email,
        params.lang(),
    )
    .map_err(|e| {
        tracing::error!("Failed to bake badge: {}", e);
        poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "image/png")
        .body(png_data))
}

#[handler]
async fn badgeclass_png(
    Path(badgeclass): Path<String>,
    Query(params): Query<ImageParams>,
) -> poem::Result<Response> {
    let badge = Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| poem::Error::from_status(StatusCode::NOT_FOUND))?;

    let badge_path = badge_artwork(badge, params.lang());
    let badge_file = crate::Static::get(&badge_path)
        .ok_or_else(|| anyhow!("Badge template not found: {}", badge_path))?;

    let png_data = badge_file.data.clone().into_owned();

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "image/png")
        .body(png_data))
}

/// The social card for a badge, which an earner's page names as its
/// `og:image`. One per badge and language rather than per earner: it says
/// what was earned, not who earned it, which the page itself never shows.
#[handler]
async fn badgeclass_card(
    Path(badgeclass): Path<String>,
    Query(params): Query<ImageParams>,
) -> poem::Result<Response> {
    let badge = Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| poem::Error::from_status(StatusCode::NOT_FOUND))?;

    let lang = params.lang();
    let shown = BadgeText::shown(badge, lang);

    let artwork_path = badge_artwork(badge, lang);
    let artwork = crate::Static::get(&artwork_path)
        .ok_or_else(|| anyhow!("Badge template not found: {}", artwork_path))?;

    let png = card::render(
        &artwork.data,
        &card_text(lang, &shown.name, shown.earn_url.as_deref()),
    )
    .map_err(|e| {
        tracing::error!("Failed to draw badge card: {}", e);
        poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "image/png")
        // Drawn from the code's own words, so it changes only with a deploy
        // or a translator's edit.
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(png))
}

/// The badge's skill alignments, in the shape Open Badges consumers read.
///
/// One conversion, used by both the hosted BadgeClass and the assertion, so
/// the two cannot drift apart.
fn alignment_json(badge: &Badge) -> Vec<AlignmentJson> {
    badge
        .alignments()
        .iter()
        .map(|a| AlignmentJson {
            target_name: a.name.to_string(),
            target_url: a.url.to_string(),
            target_description: a.description.map(|d| d.to_string()),
            target_framework: a.framework.map(|f| f.to_string()),
            target_code: a.code.map(|c| c.to_string()),
        })
        .collect()
}

fn build_badge_assertion(
    badgeclass: &str,
    user_id: &str,
    badge: &Badge,
    email: &str,
    when: &str,
) -> Result<BadgeAssertion, Error> {
    let assertion_id = format!("https://{}/badges/{}/{}", DOMAIN, badgeclass, user_id);

    Ok(BadgeAssertion {
        context: "https://w3id.org/openbadges/v2".to_string(),
        type_: "Assertion".to_string(),
        id: assertion_id.clone(),
        recipient: BadgeRecipient {
            type_: "email".to_string(),
            hashed: false,
            identity: format!("mailto:{}", email),
        },
        badge: BadgeClassEmbedded {
            context: "https://w3id.org/openbadges/v2".to_string(),
            type_: "BadgeClass".to_string(),
            id: format!("https://{}/badges/{}", DOMAIN, badgeclass),
            name: badge.name().to_string(),
            description: badge.description().to_string(),
            // The badge's image, not the page about it.
            image: badge.template_url().to_string(),
            criteria: BadgeCriteria {
                type_: "Criteria".to_string(),
                id: badge.criteria_url().to_string(),
                narrative: badge.criteria_text().to_string(),
            },
            issuer: make_issuer_json(),
            tags: Some(badge.tags().iter().map(|s| s.to_string()).collect()),
            alignment: alignment_json(badge),
        },
        issued_on: when.to_string(),
        image: format!(
            "https://{}/badges/{}/{}/badge.png",
            DOMAIN, badgeclass, user_id
        ),
        verification: BadgeVerification {
            type_: "hosted".to_string(),
        },
    })
}

/// Bake an earner's assertion into the badge artwork for `lang`.
///
/// The payload is built from [`Badge`] alone, so it is the same in every
/// language: a translated download differs from the English one only in the
/// picture it is drawn on, and validates identically.
fn bake_badge(
    badgeclass: &str,
    user_id: &str,
    badge: &Badge,
    user_badge: &UserBadge,
    email: &str,
    lang: Lang,
) -> Result<Vec<u8>, Error> {
    // The artwork is presentation, so it is chosen by language; everything
    // baked into it below comes from the badge itself.
    let badge_path = badge_artwork(*badge, lang);
    let badge_file = crate::Static::get(&badge_path)
        .ok_or_else(|| anyhow!("Badge template not found: {}", badge_path))?;

    let badge_png_data = badge_file.data;

    let when_timestamp = user_badge
        .when
        .to_string()
        .parse::<jiff::Timestamp>()
        .map_err(|e| anyhow!("Failed to parse timestamp: {}", e))?;

    let issuer = badgebuilder::v2::Issuer::default()
        .with_id(format!("https://{}/badges/issuer.json", DOMAIN))
        .with_name("SciStarter")
        .with_url(format!("https://{}/", DOMAIN))
        .with_email("info@scistarter.org")
        .with_description("SciStarter makes it easy for anyone to find and participate in authentic online and local science through citizen science, community science, and crowdsourced research projects.")
        .with_image(format!("https://{}/-static/img/logo--white_tag-rgb.svg", DOMAIN));

    let payload = badgebuilder::v2::Payload::default()
        .with_id(format!(
            "https://{}/badges/{}/{}",
            DOMAIN, badgeclass, user_id
        ))
        .with_recipient_email(email)
        .with_issued_timestamp(when_timestamp)
        .with_issuer(issuer)
        .with_image(format!(
            "https://{}/badges/{}/{}/badge.png",
            DOMAIN, badgeclass, user_id
        ))
        .with_badge_class_id(format!("https://{}/badges/{}", DOMAIN, badgeclass))
        .with_badge_class_name(badge.name())
        .with_badge_class_description(badge.description())
        .with_badge_class_image_url(badge.template_url())
        .with_badge_class_criteria(badge.criteria_url(), badge.criteria_text());

    // Use Cursor for Read + Seek on the badge PNG data
    let source = std::io::Cursor::new(badge_png_data.as_ref());
    let mut target = Vec::new();

    badgebuilder::bake(payload, source, &mut target)?;
    Ok(target)
}

#[derive(TemplateOnce)]
#[template(path = "badge/badge.stpl.html")]
struct BadgeTemplate {
    own_badge: bool,
    /// The value for `<html lang>`, and the language everything below was
    /// rendered in.
    lang: &'static str,
    /// The badge's name in the language the page is written in.
    /// Presentation only, like the rest of the page: the credential keeps
    /// its canonical English name, which is what a verifier compares
    /// against the baked image and which `linkedin_url` below still
    /// carries to LinkedIn.
    name: String,
    /// Present only on a page whose badge really is translated: it points a
    /// reader at the wording the credential carries.
    translation_notice: Option<String>,
    language_links: Vec<LanguageLink>,
    linkedin_url: String,
    badgeclass_description: String,
    badgeclass_criteria_text: String,
    alignments: Vec<Alignment>,
    tags: Vec<String>,
    image_url: String,
    /// The issue date written out in the page's language, which
    /// `strftime`'s `%B` cannot do -- it names months in English whatever
    /// the page is in.
    issued_on: String,
    earn_url: Option<String>,
    /// The earner's post, for the share buttons that take one.
    share_text: String,
    /// How a link to this page presents itself where it is shared.
    link_title: String,
    link_description: String,
    card_url: String,
    card_width: u32,
    card_height: u32,
    /// The body of an email share, which takes no link of its own.
    share_message: String,
    canonical_url: String,
    oembed_json: String,
    oembed_xml: String,
    badge_json: String,
    download: bool,
    /// The confirmation section's introduction, naming the badge.
    confirm_intro: String,
    confirm: bool,
    confirmed: bool,
    confirm_email: String,
}

async fn render_badge_html(
    badge: &Badge,
    user_badge: &UserBadge,
    email: &str,
    badgeclass: &str,
    user_id: &str,
    params: &BadgeParams,
    own_badge: bool,
) -> Result<String, Error> {
    let lang = current_lang();

    // What the reader is shown: the badge's own words translated if it has
    // been translated into their language, and otherwise as the credential
    // carries them. One decision, rather than one per field.
    let shown = BadgeText::shown(*badge, lang);

    let badgeclass_criteria_text = markdown_to_html(&shown.criteria_text);
    let badgeclass_description = markdown_to_html(&shown.description);
    let badge_url = badge_url(*badge, user_id);
    let badge_url_relative = format!("/badges/{}/{}", badgeclass, user_id);
    // Carries the page's language, so a reader downloads the artwork they
    // were shown. What is baked into that artwork is the same assertion
    // either way.
    let image_url = format!("{}/badge.png{}", badge_url_relative, image_query(lang));
    let canonical_url = badge_url.clone();

    let issued_zoned = issued_on(user_badge)?;

    // Build badge JSON for the <script> tag
    let assertion = build_badge_assertion(
        badgeclass,
        user_id,
        badge,
        email,
        &user_badge.when.to_string(),
    )?;
    // This JSON is embedded verbatim inside a <script type="application/ld+json">
    // block. serde_json does not escape '<' or '/', so a string field containing
    // "</script>" would break out of the element (stored XSS). Escape the
    // characters that are dangerous in an HTML script context as unicode
    // escapes, which are still valid JSON.
    let badge_json = serde_json::to_string_pretty(&assertion)?
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");

    // A blank address asks nothing, rather than being reported as not the
    // recipient's.
    let confirm_email = params
        .confirm
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_owned();
    let confirm = !confirm_email.is_empty();

    // Check if confirm email matches any of the user's emails
    let confirmed = if confirm {
        common::service::request(
            "user.emails.has",
            (user_id.to_string(), confirm_email.clone()),
        )
        .await
        .unwrap_or(false)
    } else {
        false
    };

    let share = share_text(&shown.name, shown.earn_url.as_deref());
    let card = card_text(lang, &shown.name, shown.earn_url.as_deref());

    let template = BadgeTemplate {
        own_badge,
        lang: lang.code(),
        name: shown.name.clone(),
        translation_notice: shown.translated.then(|| translation_notice(&canonical_url)),
        language_links: text::language_links(*badge, &canonical_url, lang),
        linkedin_url: linkedin_add_url(*badge, user_id, &issued_zoned),
        badgeclass_description,
        badgeclass_criteria_text,
        alignments: badge.alignments().to_vec(),
        tags: shown.tags,
        image_url,
        issued_on: crate::i18n::format_date(issued_zoned.date()),
        earn_url: shown.earn_url,
        share_message: format!("{share}\n\n{canonical_url}"),
        share_text: share,
        link_title: card.title(),
        link_description: link_description(&card, &shown.description),
        card_url: card_url(badgeclass, lang),
        card_width: card::WIDTH,
        card_height: card::HEIGHT,
        canonical_url: canonical_url.clone(),
        oembed_json: format!(
            "{}/oembed.json?url={}&format=json",
            badge_url,
            urlencoding::encode(&canonical_url)
        ),
        oembed_xml: format!(
            "{}/oembed.xml?url={}&format=xml",
            badge_url,
            urlencoding::encode(&canonical_url)
        ),
        badge_json,
        download: params.download.unwrap_or(false),
        confirm_intro: tr(CONFIRM_INTRO).replace("{name}", &shown.name),
        confirm,
        confirm_email,
        confirmed,
    };

    Ok(template.render_once()?)
}

#[derive(TemplateOnce)]
#[template(path = "badge/badgeclass.stpl.html")]
struct BadgeClassTemplate {
    lang: &'static str,
    /// The page's language, for the same reason as on [`BadgeTemplate`].
    name: String,
    translation_notice: Option<String>,
    language_links: Vec<LanguageLink>,
    badgeclass_description_raw: String,
    badgeclass_description: String,
    badgeclass_criteria_text: String,
    alignments: Vec<Alignment>,
    tags: Vec<String>,
    image_url: String,
    earn_url: Option<String>,
    canonical_url: String,
}

async fn render_badgeclass_html(badge: &Badge, badgeclass: &str) -> Result<String, Error> {
    let lang = current_lang();
    let shown = BadgeText::shown(*badge, lang);

    let badgeclass_criteria_text = markdown_to_html(&shown.criteria_text);
    let badgeclass_description = markdown_to_html(&shown.description);
    // "/badges/", plural: that is where the routes are mounted. This page
    // used to name itself "/badge/", so its own image link 404'd.
    let canonical_url = format!("https://{}/badges/{}", DOMAIN, badgeclass);
    let image_url = format!("{}/badge.png{}", &canonical_url, image_query(lang));

    let template = BadgeClassTemplate {
        lang: lang.code(),
        name: shown.name.clone(),
        translation_notice: shown.translated.then(|| translation_notice(&canonical_url)),
        language_links: text::language_links(*badge, &canonical_url, lang),
        badgeclass_description_raw: shown.description,
        badgeclass_description,
        badgeclass_criteria_text,
        alignments: badge.alignments().to_vec(),
        tags: shown.tags,
        image_url,
        earn_url: shown.earn_url,
        canonical_url,
    };

    Ok(template.render_once()?)
}

/// The oEmbed description of an earner's badge page: a link, with the
/// title and card its Open Graph tags give it in English. oEmbed has no
/// notion of language, so a consumer gets the badge's own words. The same
/// description scistarter.org gives.
#[derive(Serialize)]
struct OembedJson {
    version: &'static str,
    #[serde(rename = "type")]
    type_: &'static str,
    title: String,
    provider_name: &'static str,
    provider_url: String,
    cache_age: u32,
    thumbnail_url: String,
    thumbnail_width: u32,
    thumbnail_height: u32,
}

impl OembedJson {
    fn new(badge: Badge, badgeclass: &str) -> Self {
        let card = card_text(Lang::En, badge.name(), badge.earn_url());

        Self {
            version: "1.0",
            type_: "link",
            title: card.title(),
            provider_name: "SciStarter",
            provider_url: format!("https://{}/", DOMAIN),
            cache_age: 31536000,
            thumbnail_url: card_url(badgeclass, Lang::En),
            thumbnail_width: card::WIDTH,
            thumbnail_height: card::HEIGHT,
        }
    }
}

#[handler]
async fn oembed_json(
    Path((badgeclass, _user_id)): Path<(String, String)>,
) -> poem::Result<Json<OembedJson>> {
    let badge = Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| poem::Error::from_status(StatusCode::NOT_FOUND))?;

    Ok(Json(OembedJson::new(badge, &badgeclass)))
}

#[handler]
async fn oembed_xml(
    Path((badgeclass, _user_id)): Path<(String, String)>,
) -> poem::Result<Response> {
    let badge = Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| poem::Error::from_status(StatusCode::NOT_FOUND))?;

    let oembed = OembedJson::new(badge, &badgeclass);
    let escape = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };

    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8" standalone="yes"?>
<oembed>
  <type>{}</type>
  <version>{}</version>
  <title>{}</title>
  <provider_name>{}</provider_name>
  <provider_url>{}</provider_url>
  <cache_age>{}</cache_age>
  <thumbnail_url>{}</thumbnail_url>
  <thumbnail_width>{}</thumbnail_width>
  <thumbnail_height>{}</thumbnail_height>
</oembed>"#,
        oembed.type_,
        oembed.version,
        escape(&oembed.title),
        oembed.provider_name,
        escape(&oembed.provider_url),
        oembed.cache_age,
        escape(&oembed.thumbnail_url),
        oembed.thumbnail_width,
        oembed.thumbnail_height,
    );

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "text/xml")
        .body(xml))
}

/// What the earner's share buttons offer to post for a badge with a
/// training to earn it by. Plain text, not HTML. `{url}` is the training's
/// address as a reader would type it. Migration
/// `seed_badge_share_text_translations` translates it under exactly this
/// wording, as it does [`SHARE_PLAIN`].
const SHARE_INVITING: &str = "I'm happy to share that I've earned the {name} certification from @SciStarter. You can too at {url}";

/// The same for a badge with no training behind it, which is not a
/// certification and has nowhere to send anyone.
const SHARE_PLAIN: &str = "I'm happy to share that I've earned the {name} badge from @SciStarter.";

/// What the earner's share buttons offer to post about the badge called
/// `name`: a certification, with an invitation to the training at
/// `earn_url`, or a plain badge when there is no training to earn it by.
fn share_text(name: &str, earn_url: Option<&str>) -> String {
    match earn_url {
        Some(url) => tr(SHARE_INVITING)
            .replace("{name}", name)
            .replace("{url}", &readable_url(url)),
        None => tr(SHARE_PLAIN).replace("{name}", name),
    }
}

/// What a link to an earner's page says was earned, above the badge's name
/// on its social card and before it in its title: for a badge with a
/// training behind it...
const EARNED_CERTIFICATION: &str = "Certification earned";

/// ...and for one without.
const EARNED_BADGE: &str = "Badge earned";

/// The card's invitation to the training. `{url}` is as in
/// [`SHARE_INVITING`].
const EARN_YOURS: &str = "Earn yours at {url}";

/// How a link to an earner's badge page presents itself where it is
/// shared: what was earned -- a certification, with an invitation to the
/// training at `earn_url`, or a plain badge when there is no training -- in
/// `lang`. The words on its card, and its title.
fn card_text(lang: Lang, name: &str, earn_url: Option<&str>) -> CardText {
    CardText {
        kicker: tr_in(
            lang,
            match earn_url {
                Some(_) => EARNED_CERTIFICATION,
                None => EARNED_BADGE,
            },
        ),
        name: String::from(name),
        invite: earn_url.map(|url| tr_in(lang, EARN_YOURS).replace("{url}", &readable_url(url))),
    }
}

/// The description of a link carrying `card`: its invitation, if it has
/// one, ahead of the badge's own `description`, which previews often cut
/// short.
fn link_description(card: &CardText, description: &str) -> String {
    match &card.invite {
        Some(invite) => format!("{invite}. {description}"),
        None => String::from(description),
    }
}

/// Where the social card for `badgeclass` in `lang` is served. Like the
/// artwork, the card takes its language from `?lang=` alone.
fn card_url(badgeclass: &str, lang: Lang) -> String {
    format!(
        "https://{DOMAIN}/badges/{badgeclass}/card.png{}",
        image_query(lang)
    )
}

/// `url` as a reader would type it into a post: no scheme, and the site's
/// own name capitalized the way the brand writes it.
fn readable_url(url: &str) -> String {
    let bare = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    match bare.strip_prefix("scistarter.org") {
        Some(path) => format!("SciStarter.org{path}"),
        None => bare.to_owned(),
    }
}

/// The courtesy-translation notice for a page whose badge is translated.
///
/// It says which language the credential itself is in and links to it, so a
/// reader who needs the official wording -- the wording a verifier will show
/// them -- can get to it in one click.
fn translation_notice(canonical_url: &str) -> String {
    tr(
        r#"This is a courtesy translation. The official text of this badge, which is what the verifiable record contains, is in English: <a class=hyperlink href="{url}">see the original</a>."#,
    )
    .replace("{url}", &text::english_url(canonical_url))
}

pub fn markdown_to_html(markdown: &str) -> String {
    use markdown_ppp::html_printer::config::Config;
    use markdown_ppp::html_printer::render_html;
    use markdown_ppp::parser::{MarkdownParserState, parse_markdown};

    let config = Config::default();

    let Some(ast) = parse_markdown(MarkdownParserState::default(), markdown).ok() else {
        return String::from("Markdown parsing failed");
    };

    render_html(&ast, config).replace("</br>", "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::timestring::TimeString;

    /// LinkedIn's "Add to Profile" link names the credential by the badge's
    /// canonical name and points back at its badge page, with the issue
    /// month LinkedIn files it under. The badge page and the training pages
    /// both hand out this link.
    #[test]
    fn the_linkedin_link_describes_the_credential() {
        let issued: jiff::Zoned = "2026-10-22T15:00:00Z"
            .parse::<Timestamp>()
            .unwrap()
            .to_zoned(jiff::tz::TimeZone::UTC);
        let user_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

        assert_eq!(
            linkedin_add_url(Badge::FoundationsOfCitizenScience, user_id, &issued),
            format!(
                "https://www.linkedin.com/profile/add?startTask=CERTIFICATION_NAME&name=Foundations%20of%20Citizen%20Science&organizationId=3205961&issueYear=2026&issueMonth=10&certUrl=https%3A%2F%2F{DOMAIN}%2Fbadges%2F6Jg_IwmfQTaJ2kUNePsO5Q%2F{user_id}&certId=6Jg_IwmfQTaJ2kUNePsO5Q%2F{user_id}"
            )
        );
    }

    /// The credential says the same thing to everyone, in every language.
    ///
    /// This is the property the whole translation design rests on -- the
    /// pages are translated, the badge is not -- so it is checked rather
    /// than trusted: nothing a translation introduces may appear in the
    /// hosted assertion or in the metadata baked into the downloadable
    /// image, which are what an Open Badges validator reads.
    ///
    /// The translations are installed here rather than read from the
    /// catalog the running site loads, so that the invariant is tested even
    /// on a database where nobody has translated anything yet -- and so that
    /// what the test looks for is text it knows the exact shape of.
    ///
    /// orrery has the same test over the same badge, because the two sites
    /// issue it under the same rules.
    #[test]
    fn no_translated_string_reaches_the_credential() {
        use crate::i18n::{Origin, Phrase, Provenance, install_for_test};

        let email = "someone@example.invalid";
        let user_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let badge = Badge::DataEthicsForPractitioners;

        // Distinctive enough that finding one in a credential is proof, and
        // not a substring of anything the badge itself says.
        let marker = |lang: Lang, what: &str| format!("ZZTRANSLATED-{}-{what}", lang.code());

        let mut sources = vec![
            ("", badge.name(), "name"),
            ("", badge.description(), "description"),
            ("", badge.criteria_text(), "criteria"),
        ];

        sources.extend(badge.tags().iter().map(|tag| ("", *tag, "tag")));

        if let Some(url) = badge.earn_url() {
            sources.push((text::EARN_URL, url, "earn-url"));
        }

        // Every language at once: a phrase is stored whole, so installing
        // them one at a time would leave only the last.
        for (context, source, what) in sources {
            let mut phrase = Phrase::new(context, source, Origin::Manual);

            for lang in Lang::TRANSLATABLE {
                phrase.set_translation(lang, marker(lang, what), Provenance::Human);
            }

            install_for_test(&phrase);
        }

        let badgeclass = badge.badgeclass();
        let user_badge = UserBadge {
            badge,
            when: TimeString::now(),
        };

        // The badge really is translated now, or the rest of this would pass
        // by finding nothing to look for.
        for lang in Lang::TRANSLATABLE {
            let shown = BadgeText::shown(badge, lang);

            assert!(
                shown.translated,
                "{} is not translated, so this test proves nothing",
                lang.code()
            );
        }

        let assertion = build_badge_assertion(
            badgeclass,
            user_id,
            &badge,
            email,
            &user_badge.when.to_string(),
        )
        .expect("assertion");
        let json = serde_json::to_string(&assertion).expect("assertion json");

        // Every language's artwork, since a translated badge is offered
        // for download drawn on its own picture: what is baked into it
        // has to be the same credential as the English one.
        let baked: Vec<(Lang, String)> = Lang::ALL
            .into_iter()
            .map(|lang| {
                let image = bake_badge(badgeclass, user_id, &badge, &user_badge, email, lang)
                    .expect("baked image");

                (lang, String::from_utf8_lossy(&image).into_owned())
            })
            .collect();

        // Positive control, so this can't pass by finding nothing at
        // all: the canonical text has to be in there.
        assert!(
            json.contains(badge.name()),
            "no name in the assertion for {badgeclass}"
        );

        for (lang, image) in &baked {
            assert!(
                image.contains(badge.name()),
                "no name in the {} baked image for {badgeclass}",
                lang.code()
            );
        }

        for lang in Lang::TRANSLATABLE {
            for what in ["name", "description", "criteria", "tag", "earn-url"] {
                let translated = marker(lang, what);

                assert!(
                    !json.contains(&translated),
                    "translated text reached the assertion for {badgeclass}: {translated:?}"
                );

                for (baked_lang, image) in &baked {
                    assert!(
                        !image.contains(&translated),
                        "translated text reached the {} baked image for {badgeclass}: {translated:?}",
                        baked_lang.code()
                    );
                }
            }
        }
    }

    /// A translated download is the same credential drawn on a different
    /// picture, so the bytes have to differ -- otherwise the page is
    /// offering artwork it isn't actually serving.
    #[test]
    fn each_language_bakes_onto_its_own_artwork() {
        let badge = Badge::DataEthicsForPractitioners;
        let user_badge = UserBadge {
            badge,
            when: TimeString::now(),
        };

        let bake = |lang| {
            bake_badge(
                badge.badgeclass(),
                "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                &badge,
                &user_badge,
                "someone@example.invalid",
                lang,
            )
            .expect("baked image")
        };

        let english = bake(Lang::En);

        for lang in Lang::TRANSLATABLE {
            assert_ne!(
                english,
                bake(lang),
                "{} baked the English artwork",
                lang.code()
            );
        }
    }

    /// The seed migrations name the badge page's own phrases by their
    /// wording, out of the compiler's reach: reword one and its phrase id
    /// moves, stranding the translation and leaving it English on a
    /// translated page.
    #[tokio::test]
    async fn the_badge_page_phrases_are_seeded_in_every_language() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let pool = sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(dir.path().join("translations.sqlite"))
                .create_if_missing(true),
        )
        .await
        .expect("pool");

        crate::i18n::create_schema_for_test(&pool).await;

        for seed in [
            // Holds the translations the share-text seed carries over to
            // the earn button's new wording.
            include_str!("../migrations/20260908180257_seed_badge_page_translations.sql"),
            include_str!("../migrations/20261022191434_seed_badge_linkedin_hint_translation.sql"),
            include_str!("../migrations/20261022195218_seed_badge_share_text_translations.sql"),
        ] {
            sqlx::raw_sql(seed).execute(&pool).await.expect("seed");
        }

        for source in [
            LINKEDIN_HINT,
            SHOWCASE_HEADING,
            SHOWCASE_INTRO,
            LINKEDIN_HEADING,
            LINKEDIN_EXPLANATION,
            SOCIAL_HEADING,
            SOCIAL_EXPLANATION,
            SHARE_INVITING,
            SHARE_PLAIN,
            CONFIRM_HEADING,
            CONFIRM_AUDIENCE,
            CONFIRM_INTRO,
            CONFIRM_BUTTON,
            EARNED_CERTIFICATION,
            EARNED_BADGE,
            EARN_YOURS,
            LEARN_HOW_TO_EARN,
        ] {
            let id = crate::i18n::phrase_id("", source);

            for lang in Lang::TRANSLATABLE {
                let code = lang.code();

                let translated = sqlx::query_scalar!(
                    r#"SELECT translated FROM translation WHERE phrase_id = ? AND lang = ?"#,
                    id,
                    code
                )
                .fetch_optional(&pool)
                .await
                .expect("query")
                .unwrap_or_default();

                assert!(
                    !translated.is_empty(),
                    "the seed has no {code} for {source:?}"
                );
            }
        }
    }

    /// The earner's post names the badge they earned, and invites readers to
    /// the training behind it when there is one.
    #[test]
    fn the_share_text_names_the_badge_and_its_training() {
        let foundations = Badge::FoundationsOfCitizenScience;

        assert_eq!(
            share_text(foundations.name(), foundations.earn_url()),
            "I'm happy to share that I've earned the Foundations of Citizen Science certification from @SciStarter. You can too at SciStarter.org/training-foundations"
        );

        assert_eq!(
            share_text(
                Badge::OneMillionActsOfScience2026.name(),
                Badge::OneMillionActsOfScience2026.earn_url()
            ),
            format!(
                "I'm happy to share that I've earned the {} badge from @SciStarter.",
                Badge::OneMillionActsOfScience2026.name()
            )
        );
    }

    #[test]
    fn a_shared_link_reads_the_way_people_type_it() {
        assert_eq!(
            readable_url("https://scistarter.org/training"),
            "SciStarter.org/training"
        );
        assert_eq!(
            readable_url("https://example.org/somewhere"),
            "example.org/somewhere"
        );
    }

    /// The credential is the default, in the type Open Badges says it is;
    /// a reader asking for something else gets what it asked for. The same
    /// table as scistarter.org's.
    #[test]
    fn the_badge_object_is_what_a_request_gets_unless_it_asks_otherwise() {
        use JsonType::{Json, LdJson};
        use Representation as R;

        assert_eq!(R::negotiate(None), R::Json(LdJson));
        assert_eq!(R::negotiate(Some("")), R::Json(LdJson));
        assert_eq!(R::negotiate(Some("application/ld+json")), R::Json(LdJson));
        assert_eq!(
            R::negotiate(Some("application/ld+json, application/json")),
            R::Json(LdJson)
        );
        assert_eq!(R::negotiate(Some("application/json")), R::Json(Json));
        assert_eq!(R::negotiate(Some("image/png")), R::Png);
        assert_eq!(R::negotiate(Some("image/*")), R::Png);
        assert_eq!(
            R::negotiate(Some(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"
            )),
            R::Html
        );
        assert_eq!(R::negotiate(Some("*/*")), R::Html);
    }

    /// A link to an earner's page says what was earned and where to earn
    /// it, and a badge with no training says only what it is.
    #[test]
    fn a_shared_link_says_what_was_earned() {
        let card = card_text(
            Lang::En,
            "Foundations of Citizen Science",
            Some("https://scistarter.org/training-foundations"),
        );

        assert_eq!(
            card.title(),
            "Certification earned: Foundations of Citizen Science"
        );
        assert_eq!(
            link_description(&card, "The badge."),
            "Earn yours at SciStarter.org/training-foundations. The badge."
        );

        let card = card_text(Lang::En, "One Million Acts of Science", None);

        assert_eq!(card.title(), "Badge earned: One Million Acts of Science");
        assert_eq!(link_description(&card, "The badge."), "The badge.");
    }
}
