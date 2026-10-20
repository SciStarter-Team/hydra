//! Counters that every instance of every service draws from without
//! repeats.
//!
//! Each counter is a key in a JetStream key-value bucket held in the NATS
//! cluster's file store, and is advanced by compare-and-swap, so two
//! instances can never take the same value however their requests
//! interleave.

use anyhow::Error;
use async_nats::jetstream::{
    self,
    kv::{self, CreateErrorKind, UpdateErrorKind},
    stream::StorageType,
};
use tokio::sync::OnceCell;

use crate::shared::nats;

const BUCKET: &str = "sequences";

static STORE: OnceCell<kv::Store> = OnceCell::const_new();

async fn store() -> Result<&'static kv::Store, Error> {
    STORE
        .get_or_try_init(|| async {
            let jetstream = jetstream::new(nats().clone());
            let config = kv::Config {
                bucket: BUCKET.to_string(),
                description: "Counters drawn from without repeats (common::sequence)".to_string(),
                history: 1,
                storage: StorageType::File,
                ..Default::default()
            };

            match jetstream.create_key_value(config).await {
                Ok(store) => Ok(store),
                Err(_) => Ok(jetstream.get_key_value(BUCKET).await?),
            }
        })
        .await
}

/// Take the next value of the counter `name`: one more than the greater of
/// the last value taken and `floor`.
///
/// `floor` is the highest value the caller can see is already in use, so
/// that a counter whose bucket was lost carries on past everything handed
/// out before.
pub async fn next(name: &str, floor: i64) -> Result<i64, Error> {
    next_in(store().await?, name, floor).await
}

async fn next_in(store: &kv::Store, name: &str, floor: i64) -> Result<i64, Error> {
    loop {
        let entry = store.entry(name).await?;

        let last = match &entry {
            Some(entry) if entry.operation == kv::Operation::Put => {
                Some(std::str::from_utf8(&entry.value)?.trim().parse::<i64>()?)
            }
            _ => None,
        };

        let value = last.map_or(floor, |last| last.max(floor)) + 1;
        let encoded = value.to_string().into();

        // Losing the race to another instance leaves the counter moved on,
        // so it is read again and the next value tried.
        match &entry {
            Some(entry) => match store.update(name, encoded, entry.revision).await {
                Ok(_) => return Ok(value),
                Err(err) if err.kind() == UpdateErrorKind::WrongLastRevision => continue,
                Err(err) => return Err(err.into()),
            },
            None => match store.create(name, encoded).await {
                Ok(_) => return Ok(value),
                Err(err) if err.kind() == CreateErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err.into()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_nats::Server;
    use assert2::check;
    use std::collections::HashSet;

    async fn bucket(server: &Server) -> kv::Store {
        let jetstream = server.jetstream().await;
        let config = kv::Config {
            bucket: BUCKET.to_string(),
            history: 1,
            storage: StorageType::File,
            ..Default::default()
        };
        match jetstream.create_key_value(config).await {
            Ok(store) => store,
            Err(_) => jetstream.get_key_value(BUCKET).await.unwrap(),
        }
    }

    #[tokio::test]
    #[ignore = "needs nats-server on PATH"]
    async fn a_counter_starts_past_its_floor_and_never_goes_back() {
        let server = Server::start().await;
        let store = bucket(&server).await;

        check!(next_in(&store, "count", 100).await.unwrap() == 101);
        check!(next_in(&store, "count", 100).await.unwrap() == 102);
        check!(next_in(&store, "count", 500).await.unwrap() == 501);
        check!(next_in(&store, "count", 0).await.unwrap() == 502);
        check!(next_in(&store, "other", 0).await.unwrap() == 1);
    }

    #[tokio::test]
    #[ignore = "needs nats-server on PATH"]
    async fn racing_instances_never_take_the_same_value() {
        let server = Server::start().await;
        let first = bucket(&server).await;
        let second = bucket(&server).await;

        let draws = (0..100).map(|n| {
            let store = if n % 2 == 0 {
                first.clone()
            } else {
                second.clone()
            };
            tokio::spawn(async move { next_in(&store, "race", 0).await.unwrap() })
        });

        let values: Vec<i64> = futures::future::join_all(draws)
            .await
            .into_iter()
            .map(Result::unwrap)
            .collect();
        let distinct: HashSet<i64> = values.iter().copied().collect();

        check!(distinct.len() == 100);
        check!(distinct == (1..=100).collect::<HashSet<i64>>());
    }
}
