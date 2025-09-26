use anyhow::Error;
use async_nats::service::Request;
use common::{message::sync::NextSequenceResponse, shared::db};

async fn next_sequence_id(sequence_name: &str) -> Result<i64, Error> {
    let db = db();

    // Use a transaction to ensure atomicity
    let mut tx = db.begin().await?;

    // Get the current value and increment it
    let result = sqlx::query!(
        "UPDATE sequences SET current_value = current_value + 1 WHERE name = ? RETURNING current_value",
        sequence_name
    )
    .fetch_optional(&mut *tx)
    .await?;

    let next_id = match result {
        Some(row) => row.current_value,
        None => {
            // If sequence doesn't exist, return an error
            return Err(anyhow::anyhow!("Sequence '{}' not found", sequence_name));
        }
    };

    tx.commit().await?;

    Ok(next_id)
}

pub async fn next_project_id(_request: &Request) -> Result<NextSequenceResponse, Error> {
    let next_id = next_sequence_id("project_id").await?;
    Ok(NextSequenceResponse { next_id })
}
