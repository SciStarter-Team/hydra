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

mod text;

use crate::i18n::{Lang, current_lang, tr};
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

pub fn routes() -> Route {
    Route::new()
        .at("/issuer.json", poem::get(issuer_json))
        .at("/:badgeclass", poem::get(badge_class))
        .at("/:badgeclass/badge.png", poem::get(badgeclass_png))
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
async fn issuer_json() -> Json<IssuerJson> {
    Json(make_issuer_json())
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

    let accept_header = req
        .headers()
        .get(header::ACCEPT)
        .and_then(|h| h.to_str().ok());

    if let Some(accept_str) = accept_header {
        if accept_str.contains("text/html") {
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
    }

    Ok(Json(BadgeClassJson {
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
    })
    .into_response()
    .with_header("Vary", "Accept")
    .into_response())
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

    if !user_info.verified && !user_info.badge_allowed {
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

    // Check Accept header for content negotiation
    let accept_header = req
        .headers()
        .get(header::ACCEPT)
        .and_then(|h| h.to_str().ok());

    if let Some(accept_str) = accept_header {
        if accept_str.contains("application/json") {
            // Return JSON assertion
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
            return Ok(Json(assertion)
                .into_response()
                .with_header("Vary", "Accept")
                .into_response());
        } else if accept_str.contains("image/png") {
            // Return baked PNG
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
    }

    // Default: return HTML page
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
    Path((badgeclass, user_id)): Path<(String, String)>,
) -> poem::Result<Json<BadgeAssertion>> {
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

    Ok(Json(assertion))
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
    /// against the baked image and which `name_urlencoded` below still
    /// carries to LinkedIn.
    name: String,
    /// Present only on a page whose badge really is translated: it points a
    /// reader at the wording the credential carries.
    translation_notice: Option<String>,
    language_links: Vec<LanguageLink>,
    name_urlencoded: String,
    badge_url_urlencoded: String,
    cert_id_urlencoded: String,
    badgeclass_description_raw: String,
    badgeclass_description: String,
    badgeclass_criteria_text: String,
    alignments: Vec<Alignment>,
    tags: Vec<String>,
    image_url: String,
    /// The issue date written out in the page's language, which
    /// `strftime`'s `%B` cannot do -- it names months in English whatever
    /// the page is in.
    issued_on: String,
    issued_year: i16,
    issued_month: i8,
    earn_url: Option<String>,
    canonical_url: String,
    oembed_json: String,
    oembed_xml: String,
    badge_json: String,
    download: bool,
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
    let badge_url = format!("https://{}/badges/{}/{}", DOMAIN, badgeclass, user_id);
    let badge_url_relative = format!("/badges/{}/{}", badgeclass, user_id);
    // Carries the page's language, so a reader downloads the artwork they
    // were shown. What is baked into that artwork is the same assertion
    // either way.
    let image_url = format!("{}/badge.png{}", badge_url_relative, image_query(lang));
    let canonical_url = badge_url.clone();

    // Parse the timestamp
    let issued = user_badge
        .when
        .to_string()
        .parse::<Timestamp>()
        .map_err(|e| anyhow!("Failed to parse timestamp: {}", e))?;

    // Convert to Zoned to extract year and month
    let issued_zoned = issued.to_zoned(jiff::tz::TimeZone::UTC);
    let issued_year = issued_zoned.year();
    let issued_month = issued_zoned.month();

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

    let confirm = params.confirm.is_some();
    let confirm_email = params.confirm.clone().unwrap_or_default();

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

    let template = BadgeTemplate {
        own_badge,
        lang: lang.code(),
        name: shown.name.clone(),
        translation_notice: shown.translated.then(|| translation_notice(&canonical_url)),
        language_links: text::language_links(*badge, &canonical_url, lang),
        // Canonical on purpose: this string becomes the name of the
        // credential on the earner's LinkedIn profile, so it has to be the
        // name the badge itself carries.
        name_urlencoded: urlencoding::encode(badge.name()).to_string(),
        badge_url_urlencoded: urlencoding::encode(&badge_url).to_string(),
        cert_id_urlencoded: urlencoding::encode(&format!("{}/{}", badgeclass, user_id)).to_string(),
        badgeclass_description_raw: shown.description,
        badgeclass_description,
        badgeclass_criteria_text,
        alignments: badge.alignments().to_vec(),
        tags: shown.tags,
        image_url,
        issued_on: crate::i18n::format_date(issued_zoned.date()),
        issued_year,
        issued_month,
        earn_url: shown.earn_url,
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

#[derive(Serialize)]
struct OembedJson {
    version: String,
    #[serde(rename = "type")]
    type_: String,
    title: String,
    author_name: String,
    author_url: String,
    provider_name: String,
    provider_url: String,
    thumbnail_url: String,
    thumbnail_width: u32,
    thumbnail_height: u32,
}

#[handler]
async fn oembed_json(
    Path((badgeclass, user_id)): Path<(String, String)>,
) -> poem::Result<Json<OembedJson>> {
    let badge = Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| poem::Error::from_status(StatusCode::NOT_FOUND))?;

    Ok(Json(OembedJson {
        version: "1.0".to_string(),
        type_: "photo".to_string(),
        title: format!("{} - SciStarter Badge", badge.name()),
        author_name: "SciStarter".to_string(),
        author_url: format!("https://{}/", DOMAIN),
        provider_name: "SciStarter".to_string(),
        provider_url: format!("https://{}/", DOMAIN),
        thumbnail_url: format!("https://{}/badges/{}/{}", DOMAIN, badgeclass, user_id),
        thumbnail_width: 512,
        thumbnail_height: 512,
    }))
}

#[handler]
async fn oembed_xml(Path((badgeclass, user_id)): Path<(String, String)>) -> poem::Result<Response> {
    let badge = Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| poem::Error::from_status(StatusCode::NOT_FOUND))?;

    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<oembed>
    <version>1.0</version>
    <type>photo</type>
    <title>{} - SciStarter Badge</title>
    <author_name>SciStarter</author_name>
    <author_url>https://{}/</author_url>
    <provider_name>SciStarter</provider_name>
    <provider_url>https://{}/</provider_url>
    <thumbnail_url>https://{}/badges/{}/{}</thumbnail_url>
    <thumbnail_width>512</thumbnail_width>
    <thumbnail_height>512</thumbnail_height>
</oembed>"#,
        badge.name(),
        DOMAIN,
        DOMAIN,
        DOMAIN,
        badgeclass,
        user_id
    );

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/xml+oembed")
        .body(xml))
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
}
