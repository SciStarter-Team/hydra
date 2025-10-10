use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use jiff::Zoned;
use std::num::NonZeroU32;
use std::sync::{Arc, LazyLock};
use tokio::sync::RwLock;

/// Global rate limiter for email sending
static RATE_LIMITER: LazyLock<Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>> =
    LazyLock::new(|| {
        let rate_per_second: u32 = option_env!("EMAIL_SEND_RATE_PER_SECOND")
            .and_then(|s| s.parse().ok())
            .unwrap_or(14);

        let quota = Quota::per_second(NonZeroU32::new(rate_per_second).unwrap());
        Arc::new(RateLimiter::direct(quota))
    });

/// Daily quota tracker
struct QuotaTracker {
    quota_limit: u32,
    sent_today: Arc<RwLock<u32>>,
    last_reset: Arc<RwLock<Zoned>>,
}

impl QuotaTracker {
    fn new() -> Self {
        let quota_limit: u32 = option_env!("EMAIL_DAILY_QUOTA")
            .and_then(|s| s.parse().ok())
            .unwrap_or(50000);

        Self {
            quota_limit,
            sent_today: Arc::new(RwLock::new(0)),
            last_reset: Arc::new(RwLock::new(Zoned::now())),
        }
    }

    /// Wait until we can send (respecting both rate limit and daily quota)
    async fn wait_until_ready(&self) {
        self.check_and_reset().await;

        // Check if we're within daily quota
        let sent = self.sent_today.read().await;
        if *sent >= self.quota_limit {
            drop(sent);

            // Quota reached - wait until next day (midnight UTC)
            let now = Zoned::now();
            let tomorrow_date = now.date().tomorrow().unwrap();
            let tomorrow_midnight = tomorrow_date.at(0, 0, 0, 0).to_zoned(jiff::tz::TimeZone::UTC).unwrap();
            let duration = tomorrow_midnight.duration_since(&now);

            tracing::info!(
                "Daily quota reached, waiting until {} UTC",
                tomorrow_midnight
            );

            tokio::time::sleep(duration.try_into().unwrap()).await;
            self.check_and_reset().await;
        }

        // Wait for rate limiter
        RATE_LIMITER.until_ready().await;
    }

    /// Increment the sent count (call after successfully sending)
    async fn increment(&self) {
        self.check_and_reset().await;
        let mut sent = self.sent_today.write().await;
        *sent += 1;
    }

    /// Get remaining quota for today
    async fn remaining(&self) -> u32 {
        self.check_and_reset().await;
        let sent = self.sent_today.read().await;
        self.quota_limit.saturating_sub(*sent)
    }

    /// Check if we need to reset the counter (new day in UTC)
    async fn check_and_reset(&self) {
        let now = Zoned::now();
        let mut last_reset = self.last_reset.write().await;

        // Check if it's a new day (UTC)
        if now.date() != last_reset.date() {
            tracing::info!("Resetting daily quota counter");
            let mut sent = self.sent_today.write().await;
            *sent = 0;
            *last_reset = now;
        }
    }
}

/// Global quota tracker instance
static QUOTA_TRACKER: LazyLock<QuotaTracker> = LazyLock::new(QuotaTracker::new);

/// Wait until we can send an email (respecting both rate limit and daily quota)
/// Call this before attempting to send each email
pub async fn wait_until_can_send() {
    QUOTA_TRACKER.wait_until_ready().await;
}

/// Mark an email as sent (increments daily quota counter)
/// Call this after successfully sending an email
pub async fn mark_sent() {
    QUOTA_TRACKER.increment().await;
}

/// Get remaining quota for today
pub async fn remaining_quota() -> u32 {
    QUOTA_TRACKER.remaining().await
}
