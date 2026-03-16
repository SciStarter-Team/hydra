use common::{misc, shared::db};
use regex::Regex;
use serde::Deserialize;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::time::sleep;
use ulid::Ulid;

#[derive(Debug, Deserialize)]
struct UnsubscribeData {
    id: i32,
    email: String,
    reason: i16,
    when: String,
}

pub async fn synchronize_unsubscribes() {
    loop {
        let last: String = misc::get("unsubscribe-sync-bookmark-when")
            .await
            .expect("Retrieving sync bookmark failed")
            .unwrap_or_else(|| String::from("1900-01-01T00:00:00Z"));
        let last_id: i32 = misc::get("unsubscribe-sync-last-id")
            .await
            .expect("Retrieving sync last_id failed")
            .unwrap_or(0);

        let client = reqwest::Client::new();

        let unsubs: Vec<UnsubscribeData> = match client
            .get("https://scistarter.org/staff/sync/unsubscribes")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&[("since", &last), ("last_id", &last_id.to_string())])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => {
                    let response_text = match resp.text().await {
                        Ok(text) => text,
                        Err(e) => {
                            eprintln!("Failed to read response body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    };

                    match serde_json::from_str(&response_text) {
                        Ok(data) => data,
                        Err(e) => {
                            eprintln!("JSON parsing error: {}", e);
                            eprintln!("Response body: {}", response_text);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("HTTP error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Request error: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let mut bookmark = last.clone();
        let mut bookmark_id = last_id;

        let got_unsubs = unsubs.len();

        for unsub in unsubs {
            if unsub.when > bookmark || (unsub.when == bookmark && unsub.id > bookmark_id) {
                bookmark = unsub.when.clone();
                bookmark_id = unsub.id;
            }

            let reason = match unsub.reason {
                1 => "bounce",
                2 => "complaint",
                3 => "unsubscribe",
                _ => "unsubscribe",
            };

            if let Err(e) = sqlx::query!(
                r#"INSERT OR IGNORE INTO do_not_send (email_address, reason, suppressed_at)
                   VALUES (?, ?, ?)"#,
                unsub.email,
                reason,
                unsub.when,
            )
            .execute(db())
            .await
            {
                eprintln!("Failed to insert unsubscribe for {}: {}", unsub.email, e);
            }
        }

        misc::set("unsubscribe-sync-bookmark-when", bookmark)
            .await
            .expect("Saving sync bookmark failed");
        misc::set("unsubscribe-sync-last-id", bookmark_id)
            .await
            .expect("Saving sync last_id failed");

        if got_unsubs == 500 {
            sleep(Duration::from_secs(60)).await;
        } else {
            sleep(Duration::from_secs(60 * 60)).await;
        }
    }
}

#[derive(Debug, Deserialize)]
struct MessageTemplateData {
    id: i32,
    slug: String,
    #[allow(dead_code)]
    kind: i16,
    subject: String,
    body: String,
    #[allow(dead_code)]
    from_address: String,
}

static LEGACY_MERGE_FIELD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{([a-zA-Z_][a-zA-Z0-9_]*)\}").expect("Invalid regex"));

fn convert_merge_syntax(html: &str) -> String {
    LEGACY_MERGE_FIELD_RE
        .replace_all(html, |caps: &regex::Captures| {
            format!("{{{{{}}}}}", caps[1].to_uppercase())
        })
        .into_owned()
}

pub async fn synchronize_message_templates() {
    loop {
        let last_id: i32 = misc::get("template-sync-last-id")
            .await
            .expect("Retrieving template sync last_id failed")
            .unwrap_or(0);

        let client = reqwest::Client::new();

        let templates: Vec<MessageTemplateData> = match client
            .get("https://scistarter.org/staff/sync/message_templates")
            .bearer_auth(env!("SYNC_TOKEN"))
            .query(&[("last_id", &last_id.to_string())])
            .send()
            .await
        {
            Ok(response) => match response.error_for_status() {
                Ok(resp) => {
                    let response_text = match resp.text().await {
                        Ok(text) => text,
                        Err(e) => {
                            eprintln!("Failed to read template sync response body: {}", e);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    };

                    match serde_json::from_str(&response_text) {
                        Ok(data) => data,
                        Err(e) => {
                            eprintln!("Template sync JSON parsing error: {}", e);
                            eprintln!("Response body: {}", response_text);
                            sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Template sync HTTP error: {}", e);
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("Template sync request error: {}", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let mut bookmark_id = last_id;
        let got_templates = templates.len();

        for template in templates {
            if template.id > bookmark_id {
                bookmark_id = template.id;
            }

            // Check if already imported
            let existing = sqlx::query!(
                "SELECT oneoff_email_id FROM oneoff_emails WHERE legacy_slug = ?",
                template.slug
            )
            .fetch_optional(db())
            .await;

            match existing {
                Ok(Some(_)) => continue,
                Err(e) => {
                    eprintln!("Failed to check existing template {}: {}", template.slug, e);
                    continue;
                }
                Ok(None) => {}
            }

            // Convert merge field syntax: {fieldname} → {{FIELDNAME}}
            let converted_html = convert_merge_syntax(&template.body);

            // Convert HTML body to Quill Delta
            let body_delta = match common::quill::html_to_delta(&converted_html) {
                Ok(delta) => delta,
                Err(e) => {
                    eprintln!(
                        "Failed to convert template {} HTML to delta: {}",
                        template.slug, e
                    );
                    continue;
                }
            };

            let oneoff_email_id = Ulid::new().to_string();
            let now = common::timestring::TimeString::now().to_string();
            let datasets_json = "[]";

            if let Err(e) = sqlx::query!(
                r#"INSERT INTO oneoff_emails (oneoff_email_id, subject, body_delta, datasets, created_at, archived, legacy_slug)
                   VALUES (?, ?, ?, ?, ?, 0, ?)"#,
                oneoff_email_id,
                template.subject,
                body_delta,
                datasets_json,
                now,
                template.slug,
            )
            .execute(db())
            .await
            {
                eprintln!(
                    "Failed to insert template {}: {}",
                    template.slug, e
                );
            } else {
                println!("Inserted template: {}", template.slug);
            }
        }

        misc::set("template-sync-last-id", bookmark_id)
            .await
            .expect("Saving template sync last_id failed");

        if got_templates == 500 {
            sleep(Duration::from_secs(60)).await;
        } else {
            sleep(Duration::from_secs(60 * 60)).await;
        }
    }
}
