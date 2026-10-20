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
/// Each election is held on the subject `{stream name in lowercase}.{key}`,
/// which is the only form of subject the stream binds, so `key` must be a
/// single subject token.
///
/// # Arguments
/// * `stream_name` - The JetStream stream name (e.g., "TASK_CLAIMS")
/// * `key` - Names this election within the stream (e.g., "daily-2024-01-15")
/// * `instance_id` - A unique identifier for this instance
/// * `election_term` - How long to retain election results (e.g., Duration::from_secs(86400) for 1 day)
///
/// # Returns
/// * `Ok(true)` if this instance was elected (published the first message)
/// * `Ok(false)` if another instance was elected first
/// * `Err(_)` if `key` is not a single subject token, or there was a
///   communication error
pub async fn elect_instance(
    stream_name: &str,
    key: &str,
    instance_id: &str,
    election_term: StdDuration,
) -> Result<bool, Error> {
    let jetstream = async_nats::jetstream::new(nats().clone());
    elect_instance_in(&jetstream, stream_name, key, instance_id, election_term).await
}

async fn elect_instance_in(
    jetstream: &async_nats::jetstream::Context,
    stream_name: &str,
    key: &str,
    instance_id: &str,
    election_term: StdDuration,
) -> Result<bool, Error> {
    let subject = election_subject(stream_name, key)?;
    let our_claim = format!("claim:{}", instance_id);

    // Ensure the stream exists with appropriate configuration
    let stream_config = async_nats::jetstream::stream::Config {
        name: stream_name.to_string(),
        subjects: vec![format!("{}.*", stream_prefix(stream_name))],
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
        filter_subject: subject.clone(),
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
    let publish_result = jetstream.publish(subject, claim_bytes.into()).await?;

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

/// The first token of every subject bound by the stream `stream_name`.
fn stream_prefix(stream_name: &str) -> String {
    stream_name.to_lowercase()
}

/// The subject of the election `key` within the stream `stream_name`.
fn election_subject(stream_name: &str, key: &str) -> Result<String, Error> {
    let single_token = !key.is_empty()
        && !key
            .chars()
            .any(|c| c == '.' || c == '*' || c == '>' || c.is_whitespace());
    if !single_token {
        anyhow::bail!("election key {key:?} is not a single subject token");
    }
    Ok(format!("{}.{key}", stream_prefix(stream_name)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_nats::Server;
    use assert2::check;

    #[test]
    fn the_election_subject_is_bound_by_the_stream() {
        check!(
            election_subject("TASK_CLAIMS", "expired-status-2026-10-20").unwrap()
                == "task_claims.expired-status-2026-10-20"
        );
    }

    #[test]
    fn a_key_must_be_one_subject_token() {
        for key in ["", "a.b", "a*", ">", "a b"] {
            check!(election_subject("TASK_CLAIMS", key).is_err(), "{key:?}");
        }
    }

    const TERM: StdDuration = StdDuration::from_secs(60 * 60);

    #[tokio::test]
    #[ignore = "needs nats-server on PATH"]
    async fn racing_instances_elect_exactly_one() {
        let server = Server::start().await;
        let mut candidates = Vec::new();
        for n in 0..10 {
            candidates.push((server.jetstream().await, format!("instance-{n}")));
        }

        let votes = candidates.iter().map(|(jetstream, id)| async move {
            elect_instance_in(jetstream, "TASK_CLAIMS", "daily-2026-10-20", id, TERM)
                .await
                .unwrap()
        });
        let elected: Vec<bool> = futures::future::join_all(votes).await;
        check!(elected.iter().filter(|won| **won).count() == 1);

        // The election stays decided for anyone who asks again.
        let winner = elected.iter().position(|won| *won).unwrap();
        for (n, (jetstream, id)) in candidates.iter().enumerate() {
            let again = elect_instance_in(jetstream, "TASK_CLAIMS", "daily-2026-10-20", id, TERM)
                .await
                .unwrap();
            check!(again == (n == winner), "{id}");
        }
    }

    #[tokio::test]
    #[ignore = "needs nats-server on PATH"]
    async fn each_key_is_a_separate_election() {
        let server = Server::start().await;
        let jetstream = server.jetstream().await;

        for key in ["daily-2026-10-20", "daily-2026-10-21"] {
            check!(
                elect_instance_in(&jetstream, "TASK_CLAIMS", key, "first", TERM)
                    .await
                    .unwrap()
            );
            check!(
                !elect_instance_in(&jetstream, "TASK_CLAIMS", key, "second", TERM)
                    .await
                    .unwrap()
            );
        }
    }
}
