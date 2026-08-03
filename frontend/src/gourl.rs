use crate::middleware::analytics::{Device, Interaction};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use anyhow::{Context, Result};
use common::message::user::{
    BasicInfo, WorkflowProgressRequest, WorkflowProgressResponse, WorkflowType,
};
use common::service::log_clickthrough;
use poem::{
    Body, IntoResponse, Response, handler,
    http::{StatusCode, header::CONTENT_TYPE},
    session::Session,
    web::{Path, Query, RealIp},
};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::sync::Arc;
use tracing::{error, info};
use ulid::Ulid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GoUrlType {
    TrackedFile { file_id: Ulid },
    Redirect { target_url: String },
    // Future types can be added here:
    // Project { project_id: Ulid },
    // Event { event_id: Ulid },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoUrl {
    pub slug: String,        // The friendly URL slug (e.g., "kittens-and-puppies")
    pub url_type: GoUrlType, // What this URL points to
    pub statemachine: String,
    pub transition: String,
    pub session_set_key: String,
    pub session_set_value: String,
}

impl LoadStorePeered for GoUrl {
    type DatabaseKey = String; // The slug
    const GROUP: &str = "gourl";
    const SPECIFIC: &str = "entry";

    async fn save_to_database(&self) -> Result<(), anyhow::Error> {
        use common::shared::db;

        let url_type_blob =
            flexbuffers::to_vec(&self.url_type).context("Failed to encode GoUrlType")?;

        sqlx::query!(
            r#"INSERT OR REPLACE INTO go_urls
               (slug, url_type, statemachine, transition,
                session_set_key, session_set_value)
               VALUES (?, ?, ?, ?, ?, ?)"#,
            self.slug,
            url_type_blob,
            self.statemachine,
            self.transition,
            self.session_set_key,
            self.session_set_value,
        )
        .execute(db())
        .await
        .context("Failed to save go_url to database")?;

        Ok(())
    }

    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, anyhow::Error> {
        use common::shared::db;

        // Use case-insensitive lookup
        let row = sqlx::query!(
            r#"SELECT slug, url_type, statemachine, transition,
                      session_set_key, session_set_value
               FROM go_urls
               WHERE LOWER(slug) = LOWER(?)"#,
            key
        )
        .fetch_optional(db())
        .await
        .context("Failed to query go_url from database")?
        .ok_or_else(|| anyhow::anyhow!("Go URL not found"))?;

        let url_type: GoUrlType = flexbuffers::from_slice(&row.url_type)
            .context("Failed to decode GoUrlType from database")?;

        Ok(GoUrl {
            slug: row.slug, // Return the original case from the database
            url_type,
            statemachine: row.statemachine,
            transition: row.transition,
            session_set_key: row.session_set_key,
            session_set_value: row.session_set_value,
        })
    }

    async fn delete_from_database(key: Self::DatabaseKey) -> Result<(), anyhow::Error> {
        use common::shared::db;

        sqlx::query!(r#"DELETE FROM go_urls WHERE LOWER(slug) = LOWER(?)"#, key)
            .execute(db())
            .await
            .context("Failed to delete go_url from database")?;

        Ok(())
    }
}

impl GoUrl {
    pub fn new_file_url(slug: String, file_id: Ulid) -> Self {
        Self {
            slug,
            url_type: GoUrlType::TrackedFile { file_id },
            statemachine: String::new(),
            transition: String::new(),
            session_set_key: String::new(),
            session_set_value: String::new(),
        }
    }

    pub fn new_redirect(slug: String, target_url: String) -> Self {
        Self {
            slug,
            url_type: GoUrlType::Redirect { target_url },
            statemachine: String::new(),
            transition: String::new(),
            session_set_key: String::new(),
            session_set_value: String::new(),
        }
    }

    pub fn is_valid_slug(slug: &str) -> bool {
        // Same validation as friendly URLs
        if slug.is_empty() || slug.len() > 100 {
            return false;
        }

        if slug.starts_with('-') || slug.ends_with('-') {
            return false;
        }

        slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    }

    /// Whether a string is usable as a redirect target
    ///
    /// A target is either absolute with an explicit scheme (`https://…`,
    /// `mailto:…`) or a path on this site beginning with a single `/`. Anything
    /// starting with a bare hostname (`example.com/x`) or a dot is rejected:
    /// the browser would read it as relative to `/go/`, which is never what was
    /// meant. A leading `//` is rejected too --- it is protocol-relative to
    /// another host rather than a path here, and is far more often a typo than
    /// an intention.
    pub fn is_valid_target(target: &str) -> bool {
        let target = target.trim();

        if let Some(path) = target.strip_prefix('/') {
            return !path.starts_with('/');
        }

        // A scheme is an ASCII letter followed by letters, digits, `+`, `-` or
        // `.`, and then a colon.
        let mut chars = target.chars();
        if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        let rest = chars.as_str();
        let scheme_len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
            .unwrap_or(rest.len());
        rest[scheme_len..].starts_with(':')
    }

    /// Check if a slug exists in the database (case-insensitive)
    /// More efficient than loading the full object for uniqueness checks
    pub async fn slug_exists(slug: &str) -> Result<bool, anyhow::Error> {
        use common::shared::db;

        let row = sqlx::query!(
            r#"SELECT slug FROM go_urls WHERE LOWER(slug) = LOWER(?) LIMIT 1"#,
            slug
        )
        .fetch_optional(db())
        .await
        .context("Failed to check if slug exists")?;

        Ok(row.is_some())
    }
}

#[derive(Serialize, Deserialize)]
pub struct ProgramQuery {
    pub program: Option<String>, // ULID as string
}

/// Advance a training when a signed-in user visits a tracked go link.
///
/// Mirrors orrery's `counted_redirect_later`: the link carries the workflow
/// (`statemachine`) and the step to record (`transition`). Some trainings have
/// no other completion signal — the K-12 tutorial has no final quiz, so this
/// visit is the only thing that marks it done. Failures are logged and
/// swallowed; the user still gets their redirect.
async fn record_tracked_transition(go_url: &GoUrl, user_info: Option<&Arc<BasicInfo>>) {
    if go_url.statemachine.is_empty() || go_url.transition.is_empty() {
        return;
    }

    let Some(user) = user_info else {
        return;
    };

    let Some(workflow) = WorkflowType::from_statemachine(&go_url.statemachine) else {
        error!(
            "Go link {} names unknown state machine {:?}",
            go_url.slug, go_url.statemachine
        );
        return;
    };

    let recorded: Result<WorkflowProgressResponse, _> = common::service::request(
        "user.workflow.progress",
        WorkflowProgressRequest {
            user_id: user.user_id.to_string(),
            workflow,
            step: go_url.transition.clone(),
        },
    )
    .await;

    if let Err(e) = recorded {
        error!(
            "Failed to record {} step {:?} for {} via go link {}: {}",
            workflow.slug(),
            go_url.transition,
            user.user_id,
            go_url.slug,
            e
        );
    }
}

/// Session keys `AuthMiddleware` trusts to establish who the visitor is. A go
/// link stamping one of these would forge a login, so the passthrough refuses
/// them however the link was configured.
const AUTHENTICATION_SESSION_KEYS: [&str; 3] = ["basic_info", "auth_username", "subject"];

/// Stamp a value into the session on visit, mirroring orrery's
/// `CountedRedirect.session_set_key` / `session_set_value`.
///
/// Deliberately generic, as it is in orrery: the key is whatever the link was
/// configured with. `signup_tags` is the one hydra reads today — see
/// [`crate::session::session_tags`], which folds it into the canonical tag set.
fn apply_session_set(go_url: &GoUrl, session: &Session) {
    if go_url.session_set_key.is_empty() {
        return;
    }

    if AUTHENTICATION_SESSION_KEYS.contains(&go_url.session_set_key.as_str()) {
        error!(
            "Go link {} tried to set authentication session key {:?}; refusing",
            go_url.slug, go_url.session_set_key
        );
        return;
    }

    session.set(&go_url.session_set_key, &go_url.session_set_value);
}

#[handler]
pub async fn handle_go_url(
    req: &poem::Request,
    Authenticated(user_info): Authenticated,
    session: &Session,
    RealIp(ip_address): RealIp,
    device: Device,
    interaction: Interaction,
    Path(slug): Path<String>,
    Query(query): Query<ProgramQuery>,
) -> Result<impl IntoResponse, poem::Error> {
    // Log the access with user information if available
    if let Some(ref user) = user_info {
        info!("User {} accessing go URL: {}", user.user_id, slug);
    } else {
        info!("Anonymous user accessing go URL: {}", slug);
    }

    let query_string = req.uri().query().unwrap_or("");
    let from_path = if query_string.is_empty() {
        format!("/go/{}", slug)
    } else {
        format!("/go/{}?{}", slug, query_string)
    };

    // Look up the GoUrl entry
    match GoUrl::load(slug.clone()).await {
        Ok(go_url) => {
            record_tracked_transition(&go_url, user_info.as_ref()).await;
            apply_session_set(&go_url, session);

            match go_url.url_type {
                GoUrlType::TrackedFile { file_id } => {
                    // Handle tracked file
                    use crate::upload::STORAGE;
                    match STORAGE
                        .get_file(file_id, user_info.as_ref().map(|u| u.user_id))
                        .await
                    {
                        Ok((metadata, data)) => {
                            // Log the file access via friendly URL as clickthrough
                            let program_id = query
                                .program
                                .as_ref()
                                .map(|p| p.clone())
                                .or_else(|| Some(file_id.to_string())); // Use file_id if no program specified

                            if let Err(e) = log_clickthrough(
                                None, // when
                                ip_address
                                    .unwrap_or_else(|| IpAddr::from([0, 0, 0, 0]))
                                    .to_string(), // ip_address
                                device, // device_id
                                interaction, // interaction_id
                                user_info.as_ref().map(|u| u.user_id), // user_id
                                from_path.clone(), // from_path
                                format!("file:{}", file_id), // to_url
                                "golink", // link_type
                                program_id, // program_id
                                None, // geolocation - will be resolved
                            )
                            .await
                            {
                                error!("Failed to log file clickthrough: {}", e);
                            }

                            let content_type = metadata
                                .content_type
                                .unwrap_or_else(|| "application/octet-stream".to_string());

                            let mut response = Response::builder()
                                .status(StatusCode::OK)
                                .header(CONTENT_TYPE, content_type);

                            // Add attachment headers if force_download is true
                            if metadata.force_download {
                                response = response.header(
                                    "Content-Disposition",
                                    if let Some(filename) = &metadata.original_filename {
                                        format!("attachment; filename=\"{}\"", filename)
                                    } else {
                                        "attachment".to_string()
                                    },
                                );
                            }

                            Ok(response.body(Body::from_bytes(data.into())))
                        }
                        Err(e) => {
                            error!("Failed to retrieve file via go URL {}: {}", slug, e);
                            Err(poem::Error::from_string(
                                "File not found",
                                StatusCode::NOT_FOUND,
                            ))
                        }
                    }
                }
                GoUrlType::Redirect { target_url } => {
                    // Log the redirect access as clickthrough
                    let program_id = query.program.as_ref().map(|p| p.clone());

                    if let Err(e) = log_clickthrough(
                        None, // when
                        ip_address
                            .unwrap_or_else(|| IpAddr::from([0, 0, 0, 0]))
                            .to_string(), // ip_address
                        device, // device_id
                        interaction, // interaction_id
                        user_info.as_ref().map(|u| u.user_id), // user_id
                        from_path.clone(), // from_path
                        target_url.clone(), // to_url
                        "golink", // link_type
                        program_id, // program_id from query
                        None, // geolocation - will be resolved
                    )
                    .await
                    {
                        error!("Failed to log redirect clickthrough: {}", e);
                    }

                    // Return redirect response
                    Ok(Response::builder()
                        .status(StatusCode::FOUND)
                        .header("Location", target_url)
                        .body(Body::empty()))
                }
            }
        }
        Err(e) => {
            error!("Failed to find go URL {}: {}", slug, e);
            Err(poem::Error::from_string(
                "URL not found",
                StatusCode::NOT_FOUND,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GoUrl;

    #[test]
    fn a_target_needs_a_scheme_or_a_leading_slash() {
        for good in [
            "https://scistarter.org/finder",
            "http://example.org",
            "mailto:info@scistarter.org",
            "/training/foundations",
            "/",
            "/login?next=%2Fgirlscouts-info&tag=girlscout",
        ] {
            assert!(GoUrl::is_valid_target(good), "{good:?} should be accepted");
        }

        for bad in [
            "example.com/x",   // a browser reads this as relative to /go/
            "www.example.com", // ditto
            "./relative",
            "../up",
            ".hidden",
            "",
            "   ",
            "//evil.example", // protocol-relative to another host, not a path here
        ] {
            assert!(!GoUrl::is_valid_target(bad), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn surrounding_whitespace_does_not_change_the_verdict() {
        assert!(GoUrl::is_valid_target("  /finder  "));
        assert!(GoUrl::is_valid_target(" https://example.org "));
        assert!(!GoUrl::is_valid_target("  example.org  "));
    }
}
