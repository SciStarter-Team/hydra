use anyhow::Error;
use aws_sdk_sesv2::{
    Client as SesClient,
    config::{Credentials, Region},
    types::{Body, Content, Destination, EmailContent, Message},
};
use ulid::Ulid;

/// Initialize AWS SES client with compile-time credentials
pub async fn init_ses_client() -> Result<SesClient, Error> {
    let region = Region::new(env!("AWS_REGION"));
    let credentials = Credentials::new(
        env!("AWS_ACCESS_KEY_ID"),
        env!("AWS_SECRET_ACCESS_KEY"),
        None,
        None,
        "env",
    );

    let config = aws_sdk_sesv2::Config::builder()
        .region(region)
        .credentials_provider(credentials)
        .build();

    Ok(SesClient::from_conf(config))
}

/// Get the configured from email address
pub fn from_email() -> &'static str {
    env!("AWS_SES_FROM_EMAIL")
}

/// Send an email via AWS SES with one-click unsubscribe headers
pub async fn send_email(
    client: &SesClient,
    from_email: &str,
    to_email: &str,
    subject: &str,
    html_body: &str,
    text_body: &str,
    sending_id: Ulid,
    user_id: Option<Ulid>,
) -> Result<String, Error> {
    // Build unsubscribe URL for one-click unsubscribe (RFC 8058)
    // Use user_id if available, otherwise use email address for non-user recipients
    let unsubscribe_param = user_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| to_email.to_string());
    let unsubscribe_url = format!(
        "https://scistarter.org/messaging/emailer/unsubscribe?a={}&b={}",
        sending_id, unsubscribe_param
    );

    // Build destination
    let destination = Destination::builder().to_addresses(to_email).build();

    // Build message content
    let html_content = Content::builder()
        .data(html_body)
        .charset("UTF-8")
        .build()?;
    let text_content = Content::builder()
        .data(text_body)
        .charset("UTF-8")
        .build()?;
    let body = Body::builder()
        .html(html_content)
        .text(text_content)
        .build();
    let subject_content = Content::builder().data(subject).charset("UTF-8").build()?;
    let message = Message::builder()
        .subject(subject_content)
        .body(body)
        .build();

    let email_content = EmailContent::builder().simple(message).build();

    // Send email with custom headers for one-click unsubscribe
    let result = client
        .send_email()
        .from_email_address(from_email)
        .destination(destination)
        .content(email_content)
        // RFC 8058 one-click unsubscribe headers
        .email_tags(
            aws_sdk_sesv2::types::MessageTag::builder()
                .name("List-Unsubscribe")
                .value(format!("<{}>", unsubscribe_url))
                .build()?,
        )
        .email_tags(
            aws_sdk_sesv2::types::MessageTag::builder()
                .name("List-Unsubscribe-Post")
                .value("List-Unsubscribe=One-Click")
                .build()?,
        )
        .send()
        .await?;

    // Get the message ID from SES
    let message_id = result.message_id().unwrap_or("unknown").to_string();

    // Log the sent event
    let _ = common::service::log_email_event(
        None,
        "sent",
        sending_id,
        to_email,
        user_id,
        None::<String>,
    )
    .await;

    Ok(message_id)
}
