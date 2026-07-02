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

const DOMAIN: &str = env!("DOMAIN");

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

    let criteria_narrative = markdown_to_html(badge.criteria_text());

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
            id: format!("https://{}/badges/{}", DOMAIN, badgeclass),
            narrative: criteria_narrative,
        },
        issuer: format!("https://{}/badges/issuer.json", DOMAIN),
        tags: Some(badge.tags().iter().map(|s| s.to_string()).collect()),
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
}

#[handler]
async fn badge_instance(
    session: &Session,
    req: &poem::Request,
    Path((badgeclass, user_id)): Path<(String, String)>,
    Query(params): Query<BadgeParams>,
) -> poem::Result<Response> {
    let badge = Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| poem::Error::from_status(StatusCode::NOT_FOUND))?;

    let user_ulid = user_id
        .parse::<Ulid>()
        .map_err(|_| poem::Error::from_status(StatusCode::BAD_REQUEST))?;

    // Check if current user owns this badge
    let current_user: Option<BasicInfo> = session.get("basic_info");
    let own_badge = current_user
        .as_ref()
        .map(|u| u.user_id == user_ulid)
        .unwrap_or(false);

    // Query user-service for badge data
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

    // Get user email
    let email_request = UserEmailInfoRequest { user_id: user_ulid };

    let user_info: UserEmailInfo = common::service::request("user.info.email_info", &email_request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get user email: {}", e);
            poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    // Check if user is verified - unverified users' badges are not publicly viewable
    if !user_info.verified {
        return Err(poem::Error::from_status(StatusCode::NOT_FOUND));
    }

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
    Path((badgeclass, user_id)): Path<(String, String)>,
) -> poem::Result<Json<BadgeAssertion>> {
    let badge = Badge::from_badgeclass(&badgeclass)
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

    // Get user email
    let email_request = UserEmailInfoRequest { user_id: user_ulid };

    let user_info: UserEmailInfo = common::service::request("user.info.email_info", &email_request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get user email: {}", e);
            poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    // Check if user is verified - unverified users' badges are not publicly viewable
    if !user_info.verified {
        return Err(poem::Error::from_status(StatusCode::NOT_FOUND));
    }

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
async fn badge_png(Path((badgeclass, user_id)): Path<(String, String)>) -> poem::Result<Response> {
    let badge = Badge::from_badgeclass(&badgeclass)
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

    // Get user email
    let email_request = UserEmailInfoRequest { user_id: user_ulid };

    let user_info: UserEmailInfo = common::service::request("user.info.email_info", &email_request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get user email: {}", e);
            poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    // Check if user is verified - unverified users' badges are not publicly viewable
    if !user_info.verified {
        return Err(poem::Error::from_status(StatusCode::NOT_FOUND));
    }

    let png_data = bake_badge(
        &badgeclass,
        &user_id,
        &badge,
        &user_badge,
        &user_info.primary_email,
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
async fn badgeclass_png(Path(badgeclass): Path<String>) -> poem::Result<Response> {
    let badge = Badge::from_badgeclass(&badgeclass)
        .ok_or_else(|| poem::Error::from_status(StatusCode::NOT_FOUND))?;

    let badge_path = badge.template_path();
    let badge_file = crate::Static::get(badge_path)
        .ok_or_else(|| anyhow!("Badge template not found: {}", badge_path))?;

    let png_data = badge_file.data.clone().into_owned();

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "image/png")
        .body(png_data))
}

fn build_badge_assertion(
    badgeclass: &str,
    user_id: &str,
    badge: &Badge,
    email: &str,
    when: &str,
) -> Result<BadgeAssertion, Error> {
    let assertion_id = format!("https://{}/badges/{}/{}", DOMAIN, badgeclass, user_id);
    let criteria_narrative = markdown_to_html(badge.criteria_text());

    let alignments: Vec<AlignmentJson> = badge
        .alignments()
        .iter()
        .map(|a| AlignmentJson {
            target_name: a.name.to_string(),
            target_url: a.url.to_string(),
            target_description: a.description.map(|d| d.to_string()),
            target_framework: a.framework.map(|f| f.to_string()),
            target_code: a.code.map(|c| c.to_string()),
        })
        .collect();

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
            image: format!("https://{}/badges/{}", DOMAIN, badgeclass),
            criteria: BadgeCriteria {
                type_: "Criteria".to_string(),
                id: format!("https://{}/badges/{}", DOMAIN, badgeclass),
                narrative: criteria_narrative,
            },
            issuer: make_issuer_json(),
            tags: Some(badge.tags().iter().map(|s| s.to_string()).collect()),
            alignment: alignments,
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

fn bake_badge(
    badgeclass: &str,
    user_id: &str,
    badge: &Badge,
    user_badge: &UserBadge,
    email: &str,
) -> Result<Vec<u8>, Error> {
    // Access the embedded badge PNG from the Static struct using the Badge's template_path
    let badge_path = badge.template_path();
    let badge_file = crate::Static::get(badge_path)
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
        .with_badge_class_image_url(format!("https://{}/badges/{}", DOMAIN, badgeclass))
        .with_badge_class_criteria(
            format!("https://{}/badges/{}", DOMAIN, badgeclass),
            badge.criteria_text(),
        );

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
    name: String,
    name_urlencoded: String,
    badge_url_urlencoded: String,
    cert_id_urlencoded: String,
    badgeclass_description_raw: String,
    badgeclass_description: String,
    badgeclass_criteria_text: String,
    alignments: Vec<Alignment>,
    tags: Vec<String>,
    image_url: String,
    issued: Timestamp,
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
    let badgeclass_criteria_text = markdown_to_html(badge.criteria_text());
    let badgeclass_description = markdown_to_html(badge.description());
    let badge_url = format!("https://{}/badges/{}/{}", DOMAIN, badgeclass, user_id);
    let badge_url_relative = format!("/badges/{}/{}", badgeclass, user_id);
    let image_url = format!("{}/badge.png", badge_url_relative);
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
        name: badge.name().to_string(),
        name_urlencoded: urlencoding::encode(badge.name()).to_string(),
        badge_url_urlencoded: urlencoding::encode(&badge_url).to_string(),
        cert_id_urlencoded: urlencoding::encode(&format!("{}/{}", badgeclass, user_id)).to_string(),
        badgeclass_description_raw: badge.description().to_string(),
        badgeclass_description,
        badgeclass_criteria_text,
        alignments: badge.alignments().to_vec(),
        tags: badge.tags().iter().map(|s| s.to_string()).collect(),
        image_url,
        issued,
        issued_year,
        issued_month,
        earn_url: badge.earn_url().map(|s| s.to_string()),
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
    name: String,
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
    let badgeclass_criteria_text = markdown_to_html(badge.criteria_text());
    let badgeclass_description = markdown_to_html(badge.description());
    let canonical_url = format!("https://{}/badge/{}", DOMAIN, badgeclass);
    let image_url = format!("{}/badge.png", &canonical_url);

    let template = BadgeClassTemplate {
        name: badge.name().to_string(),
        badgeclass_description_raw: badge.description().to_string(),
        badgeclass_description,
        badgeclass_criteria_text,
        alignments: badge.alignments().to_vec(),
        tags: badge.tags().iter().map(|s| s.to_string()).collect(),
        image_url,
        earn_url: badge.earn_url().map(|s| s.to_string()),
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

fn markdown_to_html(markdown: &str) -> String {
    use markdown_ppp::html_printer::config::Config;
    use markdown_ppp::html_printer::render_html;
    use markdown_ppp::parser::{MarkdownParserState, parse_markdown};

    let config = Config::default();

    let Some(ast) = parse_markdown(MarkdownParserState::default(), markdown).ok() else {
        return String::from("Markdown parsing failed");
    };

    render_html(&ast, config).replace("</br>", "")
}
