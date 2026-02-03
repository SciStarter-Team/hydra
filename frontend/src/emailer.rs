use anyhow::Error;
use common::{
    message::email::{BounceEvent, ComplaintEvent, OpenEvent, UnsubscribeEvent},
    service::send,
};
use poem::{
    Body, Endpoint, Response, Route, get, handler,
    http::StatusCode,
    post,
    web::{Json, Query},
};
use serde::Deserialize;
use ulid::Ulid;

use crate::{
    Static,
    component::{Component, Head, StandardLayout},
};

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/bounce", post(handle_bounce))
        .at("/complaint", post(handle_complaint))
        .at(
            "/unsubscribe",
            get(show_unsubscribe).post(process_unsubscribe),
        )
        .at("/open", get(track_open))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SnsNotification {
    #[serde(rename = "Type")]
    #[allow(dead_code)]
    notification_type: String,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SesBounceNotification {
    bounce: SesBounce,
    mail: SesMail,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SesBounce {
    bounce_type: String,
    bounced_recipients: Vec<SesBouncedRecipient>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SesBouncedRecipient {
    email_address: String,
    #[allow(dead_code)]
    status: Option<String>,
    diagnostic_code: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SesComplaintNotification {
    complaint: SesComplaint,
    mail: SesMail,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SesComplaint {
    complained_recipients: Vec<SesComplainedRecipient>,
    complaint_feedback_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SesComplainedRecipient {
    email_address: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SesMail {
    message_id: String,
    #[allow(dead_code)]
    tags: Option<std::collections::HashMap<String, Vec<String>>>,
}

// ============================================================================
// Unsubscribe Templates
// ============================================================================

#[derive(sailfish::TemplateOnce)]
#[template(path = "emailer/unsubscribe_content.stpl.html")]
struct UnsubscribeContent {
    email: Option<String>,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "emailer/unsubscribed_content.stpl.html")]
struct UnsubscribedContent {
    email: Option<String>,
}

// ============================================================================
// Handlers
// ============================================================================

/// Handle AWS SES bounce notifications via SNS webhook
#[handler]
#[tracing::instrument(skip(notification))]
async fn handle_bounce(Json(notification): Json<SnsNotification>) -> Response {
    tracing::info!("Received bounce notification");

    // Parse the SNS message JSON
    let bounce_msg: Result<SesBounceNotification, _> = serde_json::from_str(&notification.message);

    match bounce_msg {
        Ok(bounce) => {
            for recipient in &bounce.bounce.bounced_recipients {
                // Look up user_id from email address
                use common::message::user::{UserDetailsListResponse, UserDetailsSearchRequest};
                use common::service::request;

                let user_id = match request::<UserDetailsListResponse>(
                    "user.admin.search",
                    UserDetailsSearchRequest {
                        search: Some(recipient.email_address.clone()),
                        role: None,
                        email_status: None,
                        page: None,
                        limit: Some(1),
                    },
                )
                .await
                {
                    Ok(users) if !users.users.is_empty() => Some(users.users[0].user_id),
                    _ => None,
                };

                let bounce_event = BounceEvent {
                    sending_id: None, // Will be looked up by emailer-service
                    ses_message_id: Some(bounce.mail.message_id.clone()),
                    user_id,
                    email_address: recipient.email_address.clone(),
                    bounce_type: bounce.bounce.bounce_type.clone(),
                    details: recipient
                        .diagnostic_code
                        .clone()
                        .unwrap_or_else(|| "No details".to_string()),
                };

                if let Err(e) = send("emailer.event.bounce", bounce_event).await {
                    tracing::error!("Failed to send bounce event: {:?}", e);
                }
            }

            Response::builder().status(StatusCode::OK).body("OK")
        }
        Err(e) => {
            tracing::error!("Failed to parse bounce notification: {:?}", e);
            Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body("Bad Request")
        }
    }
}

/// Handle AWS SES complaint notifications via SNS webhook
#[handler]
#[tracing::instrument(skip(notification))]
async fn handle_complaint(Json(notification): Json<SnsNotification>) -> Response {
    tracing::info!("Received complaint notification");

    // Parse the SNS message JSON
    let complaint_msg: Result<SesComplaintNotification, _> =
        serde_json::from_str(&notification.message);

    match complaint_msg {
        Ok(complaint) => {
            for recipient in &complaint.complaint.complained_recipients {
                // Look up user_id from email address
                use common::message::user::{UserDetailsListResponse, UserDetailsSearchRequest};
                use common::service::request;

                let user_id = match request::<UserDetailsListResponse>(
                    "user.admin.search",
                    UserDetailsSearchRequest {
                        search: Some(recipient.email_address.clone()),
                        role: None,
                        email_status: None,
                        page: None,
                        limit: Some(1),
                    },
                )
                .await
                {
                    Ok(users) if !users.users.is_empty() => Some(users.users[0].user_id),
                    _ => None,
                };

                let complaint_event = ComplaintEvent {
                    sending_id: None, // Will be looked up by emailer-service
                    ses_message_id: Some(complaint.mail.message_id.clone()),
                    user_id,
                    email_address: recipient.email_address.clone(),
                    complaint_type: complaint.complaint.complaint_feedback_type.clone(),
                };

                if let Err(e) = send("emailer.event.complaint", complaint_event).await {
                    tracing::error!("Failed to send complaint event: {:?}", e);
                }
            }

            Response::builder().status(StatusCode::OK).body("OK")
        }
        Err(e) => {
            tracing::error!("Failed to parse complaint notification: {:?}", e);
            Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body("Bad Request")
        }
    }
}

/// Unsubscribe query parameters
#[derive(Debug, Deserialize)]
struct UnsubscribeParams {
    a: String, // sending_id
    b: String, // user_id or email
}

/// Show unsubscribe confirmation page (GET)
#[handler]
#[tracing::instrument]
async fn show_unsubscribe(Query(params): Query<UnsubscribeParams>) -> Result<Response, Error> {
    // Determine if b parameter is email or user_id
    let email = if params.b.contains('@') {
        Some(params.b.clone())
    } else {
        None
    };

    let layout = StandardLayout::new(
        "/messaging/emailer/unsubscribe",
        UnsubscribeContent { email },
    )
    .prepare(None)
    .await?;

    #[derive(sailfish::TemplateOnce)]
    #[template(path = "emailer/unsubscribe.stpl.html")]
    struct UnsubscribePage {
        head: <Head as Component>::Prepared,
        layout: <StandardLayout<UnsubscribeContent> as Component>::Prepared,
    }

    UnsubscribePage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

/// Process unsubscribe action (POST)
#[handler]
#[tracing::instrument]
async fn process_unsubscribe(Query(params): Query<UnsubscribeParams>) -> Result<Response, Error> {
    // Parse sending_id
    let sending_id = params.a.parse::<Ulid>().ok();

    // Determine if b parameter is email or user_id
    let (user_id, email) = if params.b.contains('@') {
        (None, params.b.clone())
    } else {
        (params.b.parse::<Ulid>().ok(), String::new())
    };

    // Send unsubscribe event to emailer service
    let unsubscribe_event = UnsubscribeEvent {
        sending_id,
        user_id,
        email_address: email.clone(),
    };

    if let Err(e) = send("emailer.event.unsubscribe", unsubscribe_event).await {
        tracing::error!("Failed to send unsubscribe event: {:?}", e);
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body("Failed to process unsubscribe"));
    }

    // Show success page
    let email_display = if email.is_empty() { None } else { Some(email) };

    let layout = StandardLayout::new(
        "/messaging/emailer/unsubscribe",
        UnsubscribedContent {
            email: email_display,
        },
    )
    .prepare(None)
    .await?;

    #[derive(sailfish::TemplateOnce)]
    #[template(path = "emailer/unsubscribed.stpl.html")]
    struct UnsubscribedPage {
        head: <Head as Component>::Prepared,
        layout: <StandardLayout<UnsubscribedContent> as Component>::Prepared,
    }

    UnsubscribedPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

/// Email open tracking pixel (GET)
/// Returns the SciStarter logo PNG
#[handler]
#[tracing::instrument]
async fn track_open(Query(params): Query<UnsubscribeParams>) -> Response {
    // Parse sending_id
    if let Ok(sending_id) = params.a.parse::<Ulid>() {
        // Determine if b parameter is email or user_id
        if params.b.contains('@') {
            // Send open event to emailer service
            let open_event = OpenEvent {
                sending_id,
                user_id: None,
                email_address: params.b.clone(),
            };

            if let Err(e) = send("emailer.event.open", open_event).await {
                tracing::error!("Failed to send open event for email: {:?}", e);
            }
        } else if let Ok(uid) = params.b.parse::<Ulid>() {
            // b parameter is a user_id - look up the email
            use common::message::user::{UserEmailInfo, UserEmailInfoRequest};
            use common::service::request;

            if let Ok(user) = request::<UserEmailInfo>(
                "user.info.email_info",
                UserEmailInfoRequest { user_id: uid },
            )
            .await
            {
                // Send open event to emailer service
                let open_event = OpenEvent {
                    sending_id,
                    user_id: Some(uid),
                    email_address: user.primary_email,
                };

                if let Err(e) = send("emailer.event.open", open_event).await {
                    tracing::error!("Failed to send open event for uid: {:?}", e);
                }
            } else {
                tracing::error!("Failed to lookup user email: {:?}", params.b);
            }
        } else {
            tracing::warn!("Invalid b parameter in open tracking: {}", params.b);
        };
    } else {
        tracing::warn!("Invalid a parameter in open tracking: {}", params.a);
    }

    // Return PNG logo regardless of event success

    let Some(logo) = Static::get("img/logo/png/logo-email@2x.png") else {
        tracing::error!("Logo file not found in embedded assets");
        return Response::builder()
            .content_type("image/png")
            .body(Body::empty());
    };

    Response::builder()
        .content_type("image/png")
        .body(logo.data.into_owned())
}
