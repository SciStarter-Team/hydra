use anyhow::Error;
use aws_sdk_sesv2::{
    Client as SesClient,
    config::{Credentials, Region},
    primitives::Blob,
    types::{Destination, EmailContent, RawMessage},
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

    // Create MIME multipart message with proper headers
    let boundary = format!("----=_Part_{}", ulid::Ulid::new());

    // Build raw MIME message with List-Unsubscribe headers
    let raw_email = format!(
        "From: {}\r\n\
         To: {}\r\n\
         Subject: {}\r\n\
         MIME-Version: 1.0\r\n\
         List-Unsubscribe: <{}>\r\n\
         List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\
         Content-Type: multipart/alternative; boundary=\"{}\"\r\n\
         \r\n\
         --{}\r\n\
         Content-Type: text/plain; charset=UTF-8\r\n\
         Content-Transfer-Encoding: quoted-printable\r\n\
         \r\n\
         {}\r\n\
         --{}\r\n\
         Content-Type: text/html; charset=UTF-8\r\n\
         Content-Transfer-Encoding: quoted-printable\r\n\
         \r\n\
         {}\r\n\
         --{}--\r\n",
        from_email,
        to_email,
        subject,
        unsubscribe_url,
        boundary,
        boundary,
        quoted_printable_encode(text_body),
        boundary,
        quoted_printable_encode(html_body),
        boundary
    );

    // Create raw message
    let raw_message = RawMessage::builder()
        .data(Blob::new(raw_email.as_bytes()))
        .build()?;

    let email_content = EmailContent::builder().raw(raw_message).build();

    // Send email
    let result = client
        .send_email()
        .destination(destination)
        .content(email_content)
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

/// Encode text using quoted-printable encoding for email bodies
fn quoted_printable_encode(text: &str) -> String {
    let mut result = String::new();
    let mut line_len = 0;

    for byte in text.bytes() {
        // Characters that need to be encoded
        if byte == b'=' || byte < 32 || byte > 126 {
            let encoded = format!("={:02X}", byte);
            // Soft line break if we're getting close to 76 chars
            if line_len + encoded.len() > 75 {
                result.push_str("=\r\n");
                line_len = 0;
            }
            result.push_str(&encoded);
            line_len += encoded.len();
        } else {
            // Handle line breaks
            if byte == b'\n' {
                result.push_str("\r\n");
                line_len = 0;
            } else if byte == b'\r' {
                // Skip standalone CR, will be handled with LF
            } else {
                // Soft line break if needed
                if line_len >= 75 {
                    result.push_str("=\r\n");
                    line_len = 0;
                }
                result.push(byte as char);
                line_len += 1;
            }
        }
    }

    result
}
