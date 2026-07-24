//! Daily spend circuit breaker for the concierge.
//!
//! Tracks a weighted token count (input + 5x output, mirroring the Bedrock
//! price ratio) in a per-process atomic, synchronized best-effort across
//! frontend instances via a peering signal. This is a soft limit protecting
//! against runaway spend, not billing: restarts lose the in-memory count,
//! and the per-IP rate limits plus per-conversation caps bound the worst
//! case. Durable accounting happens through activity logging, and a
//! CloudWatch billing alarm is the operational backstop.

use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};

use anyhow::Error;
use async_nats::Message;
use serde::{Deserialize, Serialize};

use crate::concierge::engine::TurnUsage;
use crate::peering::LoadStorePeered;

/// Weighted-token ceiling per UTC day, across all instances. At Haiku
/// pricing this bounds concierge spend to roughly $3-4/day.
const DAILY_BUDGET: i64 = 2_000_000;

/// Weighted tokens spent so far during the current UTC day.
static SPENT_TODAY: AtomicI64 = AtomicI64::new(0);
/// UTC day (days since epoch) the counter applies to.
static DAY_KEY: AtomicU32 = AtomicU32::new(0);

fn today_key() -> u32 {
    (jiff::Timestamp::now().as_second() / 86_400) as u32
}

/// Reset the counter when the UTC day rolls over.
fn rollover() {
    let today = today_key();
    if DAY_KEY.swap(today, Ordering::Relaxed) != today {
        SPENT_TODAY.store(0, Ordering::Relaxed);
    }
}

/// Weight raw usage by the input/output price ratio.
pub fn weighted(usage: &TurnUsage) -> i64 {
    usage.input_tokens + 5 * usage.output_tokens
}

/// Whether the daily budget still has room for another turn.
pub fn available() -> bool {
    rollover();
    SPENT_TODAY.load(Ordering::Relaxed) < DAILY_BUDGET
}

/// Record local spend and notify peer instances.
pub async fn record(usage: &TurnUsage) {
    rollover();
    let amount = weighted(usage);
    SPENT_TODAY.fetch_add(amount, Ordering::Relaxed);
    if let Err(e) = Budget::transmit("usage", amount).await {
        tracing::warn!("Failed to transmit concierge budget usage to peers: {}", e);
    }
}

/// Signal-only peered type carrying cross-instance budget increments.
/// Nothing is persisted; the peering channel is used purely to keep each
/// instance's in-memory counter roughly in sync.
#[derive(Debug, Serialize, Deserialize)]
pub struct Budget;

impl LoadStorePeered for Budget {
    type DatabaseKey = ();

    const GROUP: &str = "concierge_budget";

    async fn save_to_database(&self) -> Result<(), Error> {
        Ok(())
    }

    async fn read_from_database(_key: ()) -> Result<Self, Error> {
        Ok(Budget)
    }

    async fn receive(signal: String, _specific: Option<String>, msg: Message) -> Result<(), Error> {
        if signal == "usage" {
            let amount: i64 = flexbuffers::from_slice(&msg.payload)?;
            rollover();
            SPENT_TODAY.fetch_add(amount, Ordering::Relaxed);
        }
        Ok(())
    }
}
