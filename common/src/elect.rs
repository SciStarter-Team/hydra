use crate::shared::nats;
use anyhow::Error;
use futures::StreamExt;
use std::time::Duration as StdDuration;

/// Elects one instance from multiple candidates using NATS JetStream.
///
/// This function implements a leader election algorithm using JetStream persistence.
/// Multiple instances can try to claim a task by publishing to a stream. The first
/// message in the stream for the given subject determines the elected leader.
///
/// # Arguments
/// * `stream_name` - The JetStream stream name (e.g., "TASK_CLAIMS")
/// * `subject` - The subject within the stream (e.g., "tasks.daily.claim.2024-01-15")
/// * `instance_id` - A unique identifier for this instance
/// * `election_term` - How long to retain election results (e.g., Duration::from_secs(86400) for 1 day)
///
/// # Returns
/// * `Ok(true)` if this instance was elected (published the first message)
/// * `Ok(false)` if another instance was elected first
/// * `Err(_)` if there was a communication error
pub async fn elect_instance(
    stream_name: &str,
    subject: &str,
    instance_id: &str,
    election_term: StdDuration,
) -> Result<bool, Error> {
    let jetstream = async_nats::jetstream::new(nats().clone());
    let our_claim = format!("claim:{}", instance_id);

    // Ensure the stream exists with appropriate configuration
    let stream_config = async_nats::jetstream::stream::Config {
        name: stream_name.to_string(),
        subjects: vec![format!("{}.*", stream_name.to_lowercase())],
        max_messages: 10_000,   // Reasonable limit to prevent unbounded growth
        max_age: election_term, // Use the provided election term
        storage: async_nats::jetstream::stream::StorageType::Memory,
        ..Default::default()
    };

    // Create or update the stream (idempotent operation)
    let _stream = match jetstream.create_stream(stream_config).await {
        Ok(stream) => stream,
        Err(_) => jetstream.get_stream(stream_name).await?,
    };

    // First, check if there's already a winner for this subject
    // Create a consumer to read messages from this specific subject
    let consumer_config = async_nats::jetstream::consumer::pull::Config {
        filter_subject: subject.to_string(),
        deliver_policy: async_nats::jetstream::consumer::DeliverPolicy::All,
        ack_policy: async_nats::jetstream::consumer::AckPolicy::None, // Don't remove messages when read
        ..Default::default()
    };

    let stream = jetstream.get_stream(stream_name).await?;
    let consumer = stream.create_consumer(consumer_config.clone()).await?;

    // Check if there's already a winner
    if let Ok(mut messages) = consumer.fetch().max_messages(1).messages().await {
        if let Some(message) = messages.next().await {
            let message = message.map_err(|e| anyhow::anyhow!("Error reading message: {}", e))?;
            let existing_claim = String::from_utf8_lossy(&message.payload);

            // Election already decided
            return Ok(existing_claim == our_claim);
        }
    }

    // No existing winner, try to claim by publishing
    let claim_bytes = our_claim.as_bytes().to_vec();
    let publish_result = jetstream
        .publish(subject.to_string(), claim_bytes.into())
        .await?;

    // Wait for the publish to be acknowledged
    let _ack = publish_result.await?;

    // Wait a brief moment for other potential publishers
    tokio::time::sleep(StdDuration::from_millis(100)).await;

    // Now check who won by reading the first message again
    let consumer = stream.create_consumer(consumer_config).await?;

    if let Ok(mut messages) = consumer.fetch().max_messages(1).messages().await {
        if let Some(message) = messages.next().await {
            let message = message.map_err(|e| anyhow::anyhow!("Error reading message: {}", e))?;
            let first_claim = String::from_utf8_lossy(&message.payload);

            // We won if our claim was the first message stored
            return Ok(first_claim == our_claim);
        }
    }

    // Should not happen if we just published
    Ok(false)
}
