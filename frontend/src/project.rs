use common::shared::nats;
use futures::StreamExt;

use crate::SERVER_ID_STR;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Project {}

pub async fn receive_updates() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = nats();
    let mut updates = client.subscribe("frontend.project.save").await?;
    while let Some(msg) = updates.next().await {
        if let Some(headers) = msg.headers {
            if headers
                .get("Source")
                .is_some_and(|v| v.as_str() != SERVER_ID_STR.as_str())
            {
                let project: Project = match flexbuffers::from_slice(&msg.payload) {
                    Ok(proj) => proj,
                    Err(err) => {
                        eprintln!("Unable to decode message payload to project record in receive_updates: {}", err);
                        continue;
                    }
                };
            }
        }
    }

    Ok(())
}
