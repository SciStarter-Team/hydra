use common::{misc, shared::db};
use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;

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
