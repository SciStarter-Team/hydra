//! Counting how many emails get opened, without knowing who opened them.
//!
//! A measured message loads its header logo through a URL holding a random
//! token of its own, so a load can be told from another message's load and
//! from a repeat of its own. Nothing here records who a token went to; see the
//! migration creating `email_deliveries` for what that rules out, and why.
//!
//! What a load means is limited. Apple Mail Privacy Protection loads images
//! whether or not the message is read, and clients that block images never load
//! them, so these are best compared between emails rather than read as the
//! share of people who saw one.

use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::email::{EmailRecipient, OpenRate, OpenRatesRequest, OpenRatesResponse},
    shared::db,
    timestring::TimeString,
};
use rand::{Rng, distributions::Alphanumeric};
use sqlx::SqlitePool;

const TOKEN_LENGTH: usize = 22;
const USER_AGENT_LENGTH: usize = 256;

/// A token for one message. Wholly random rather than a ULID, whose leading
/// timestamp could be lined up with `send_history`.
pub fn new_token() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(TOKEN_LENGTH)
        .map(char::from)
        .collect()
}

/// Where a message loads its logo from. Without a token the logo still loads
/// from here, and nothing is counted.
pub fn open_path(token: Option<&str>) -> String {
    match token {
        Some(token) => format!("/messaging/emailer/open?x={token}"),
        None => "/messaging/emailer/open".to_string(),
    }
}

/// Whether a sending keeps its own count of messages opened, which it can only
/// do when it was addressed to sets of people. For a sending addressed to named
/// people, as every triggered one is, that count would say what they did.
pub fn opens_by_sending(open_label: Option<&str>, recipients: &[EmailRecipient]) -> bool {
    open_label.is_some() && !recipients.is_empty() && recipients.iter().all(EmailRecipient::is_set)
}

/// Count a message as sent. `sending_id` is empty unless the sending keeps a
/// count of its own; see [`opens_by_sending`].
pub async fn record_delivery(
    pool: &SqlitePool,
    token: &str,
    label: &str,
    sending_id: Option<&str>,
) -> Result<(), Error> {
    sqlx::query!(
        r#"INSERT INTO email_deliveries (token, label, sending_id, sent_on)
           VALUES (?, ?, ?, date('now'))"#,
        token,
        label,
        sending_id
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Note a load of a message's logo. A sending's `messages_opened` goes up only
/// for a message's first load, so that it counts messages opened; a token that
/// matches no message changes nothing.
pub async fn note_open(pool: &SqlitePool, token: &str, user_agent: &str) -> Result<(), Error> {
    let now = TimeString::now();
    let user_agent: String = user_agent.chars().take(USER_AGENT_LENGTH).collect();
    let mut tx = pool.begin().await?;

    let delivery = sqlx::query!(
        r#"UPDATE email_deliveries
           SET opens = opens + 1,
               first_opened_at = COALESCE(first_opened_at, ?),
               first_open_agent = COALESCE(first_open_agent, ?)
           WHERE token = ?
           RETURNING opens, sending_id"#,
        now,
        user_agent,
        token
    )
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(delivery) = delivery
        && delivery.opens == 1
        && let Some(sending_id) = delivery.sending_id
    {
        sqlx::query!(
            r#"UPDATE sendings SET messages_opened = messages_opened + 1 WHERE sending_id = ?"#,
            sending_id
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(())
}

/// Sent and opened for each email sent in the last `days` days, busiest first.
pub async fn open_rates(pool: &SqlitePool, days: u32) -> Result<Vec<OpenRate>, Error> {
    let since = format!("-{days} days");

    let rows = sqlx::query!(
        r#"SELECT label,
                  COUNT(*) AS "delivered!: i64",
                  COUNT(first_opened_at) AS "opened!: i64"
           FROM email_deliveries
           WHERE sent_on >= date('now', ?)
           GROUP BY label
           ORDER BY 2 DESC, label"#,
        since
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| OpenRate {
            label: row.label,
            delivered: row.delivered as u32,
            opened: row.opened as u32,
        })
        .collect())
}

/// Handler for reading open rates
#[tracing::instrument(skip_all)]
pub async fn handle_open_rates(req: &Request) -> Result<OpenRatesResponse, Error> {
    let request: OpenRatesRequest = flexbuffers::from_slice(&req.message.payload)?;

    Ok(OpenRatesResponse {
        rates: open_rates(db(), request.days).await?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENDING: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";

    async fn with_a_sending() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");

        sqlx::query(
            "INSERT INTO sendings (sending_id, created_at, subject, total_queued, template_html)
             VALUES (?, '2026-11-02T00:00:00Z', 'Hello', 3, '')",
        )
        .bind(SENDING)
        .execute(&pool)
        .await
        .expect("sending");

        pool
    }

    async fn messages_opened(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT messages_opened FROM sendings WHERE sending_id = ?")
            .bind(SENDING)
            .fetch_one(pool)
            .await
            .expect("sending")
    }

    async fn delivery(pool: &SqlitePool, token: &str) -> (i64, Option<String>, Option<String>) {
        sqlx::query_as(
            "SELECT opens, first_opened_at, first_open_agent FROM email_deliveries WHERE token = ?",
        )
        .bind(token)
        .fetch_one(pool)
        .await
        .expect("delivery")
    }

    fn user_set() -> EmailRecipient {
        EmailRecipient::UserSet {
            slug: "members".to_string(),
        }
    }

    fn address_set() -> EmailRecipient {
        EmailRecipient::AddressSet {
            slug: "partners".to_string(),
        }
    }

    fn person() -> EmailRecipient {
        EmailRecipient::Email("someone@example.invalid".to_string())
    }

    /// The privacy policy speaks of usage data in aggregate. See the migration
    /// creating this table before changing this test.
    #[tokio::test]
    async fn a_delivery_has_no_way_back_to_its_recipient() {
        let pool = with_a_sending().await;

        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('email_deliveries')")
                .fetch_all(&pool)
                .await
                .expect("columns");

        assert_eq!(
            columns,
            [
                "token",
                "label",
                "sending_id",
                "sent_on",
                "opens",
                "first_opened_at",
                "first_open_agent"
            ]
        );

        // Holds no insertion order to line up with send_history.
        let without_rowid: i64 =
            sqlx::query_scalar("SELECT wr FROM pragma_table_list('email_deliveries')")
                .fetch_one(&pool)
                .await
                .expect("table");

        assert_eq!(without_rowid, 1);

        record_delivery(&pool, "abc", "welcome", None)
            .await
            .expect("delivery");

        let sent_on: String =
            sqlx::query_scalar("SELECT sent_on FROM email_deliveries WHERE token = 'abc'")
                .fetch_one(&pool)
                .await
                .expect("sent_on");

        assert_eq!(sent_on.len(), "2026-11-02".len());
    }

    #[test]
    fn a_token_is_random_and_fits_in_a_url() {
        let (one, other) = (new_token(), new_token());

        assert_ne!(one, other);
        assert_eq!(one.len(), TOKEN_LENGTH);
        assert!(one.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn the_logo_loads_from_the_same_place_measured_or_not() {
        assert_eq!(open_path(Some("abc")), "/messaging/emailer/open?x=abc");
        assert_eq!(open_path(None), "/messaging/emailer/open");
    }

    #[test]
    fn only_a_measured_send_to_sets_of_people_keeps_its_own_count() {
        let label = Some("newsletter");

        assert!(opens_by_sending(label, &[user_set(), address_set()]));

        assert!(!opens_by_sending(None, &[user_set()]));
        assert!(!opens_by_sending(label, &[person()]));
        assert!(!opens_by_sending(label, &[user_set(), person()]));
        assert!(!opens_by_sending(
            label,
            &[EmailRecipient::UserId(ulid::Ulid::new())]
        ));
        assert!(!opens_by_sending(label, &[]));
    }

    #[tokio::test]
    async fn opening_a_message_repeatedly_counts_it_once() {
        let pool = with_a_sending().await;
        record_delivery(&pool, "abc", "newsletter", Some(SENDING))
            .await
            .expect("delivery");

        for _ in 0..3 {
            note_open(&pool, "abc", "Client").await.expect("open");
        }

        assert_eq!(messages_opened(&pool).await, 1);
        assert_eq!(delivery(&pool, "abc").await.0, 3);
    }

    #[tokio::test]
    async fn opens_of_different_messages_each_count() {
        let pool = with_a_sending().await;

        for token in ["abc", "def"] {
            record_delivery(&pool, token, "newsletter", Some(SENDING))
                .await
                .expect("delivery");
            note_open(&pool, token, "Client").await.expect("open");
        }

        assert_eq!(messages_opened(&pool).await, 2);
    }

    #[tokio::test]
    async fn a_message_sent_to_a_named_person_leaves_the_sending_alone() {
        let pool = with_a_sending().await;
        record_delivery(&pool, "abc", "welcome", None)
            .await
            .expect("delivery");

        note_open(&pool, "abc", "Client").await.expect("open");

        assert_eq!(messages_opened(&pool).await, 0);
        assert_eq!(delivery(&pool, "abc").await.0, 1);

        let rates = open_rates(&pool, 30).await.expect("rates");

        assert_eq!(rates[0].label, "welcome");
        assert_eq!((rates[0].delivered, rates[0].opened), (1, 1));
    }

    #[tokio::test]
    async fn the_first_open_keeps_its_time_and_client() {
        let pool = with_a_sending().await;
        record_delivery(&pool, "abc", "newsletter", None)
            .await
            .expect("delivery");

        note_open(&pool, "abc", "First").await.expect("open");

        let (_, first_time, _) = delivery(&pool, "abc").await;

        assert!(first_time.is_some());

        sqlx::query("UPDATE email_deliveries SET first_opened_at = 'earlier'")
            .execute(&pool)
            .await
            .expect("update");

        note_open(&pool, "abc", "Second").await.expect("open");

        let (_, kept_time, kept_agent) = delivery(&pool, "abc").await;

        assert_eq!(kept_time.as_deref(), Some("earlier"));
        assert_eq!(kept_agent.as_deref(), Some("First"));
    }

    #[tokio::test]
    async fn a_long_user_agent_is_cut_short() {
        let pool = with_a_sending().await;
        record_delivery(&pool, "abc", "newsletter", None)
            .await
            .expect("delivery");

        note_open(&pool, "abc", &"x".repeat(1000))
            .await
            .expect("open");

        let (_, _, agent) = delivery(&pool, "abc").await;

        assert_eq!(agent.expect("agent").len(), USER_AGENT_LENGTH);
    }

    /// Mail that orrery sent holds tokens this table never saw, and goes on
    /// being opened after the cutover.
    #[tokio::test]
    async fn an_unknown_token_counts_nothing() {
        let pool = with_a_sending().await;
        record_delivery(&pool, "abc", "newsletter", Some(SENDING))
            .await
            .expect("delivery");

        note_open(&pool, "from-orrery", "Client")
            .await
            .expect("open");

        assert_eq!(messages_opened(&pool).await, 0);
        assert_eq!(delivery(&pool, "abc").await.0, 0);
    }

    #[tokio::test]
    async fn rates_group_by_email_busiest_first() {
        let pool = with_a_sending().await;

        for (token, label) in [("a", "reminder"), ("b", "newsletter"), ("c", "newsletter")] {
            record_delivery(&pool, token, label, None)
                .await
                .expect("delivery");
        }

        note_open(&pool, "b", "Client").await.expect("open");
        note_open(&pool, "b", "Client").await.expect("open");

        let rates = open_rates(&pool, 30).await.expect("rates");
        let rates: Vec<_> = rates
            .iter()
            .map(|rate| (rate.label.as_str(), rate.delivered, rate.opened))
            .collect();

        assert_eq!(rates, [("newsletter", 2, 1), ("reminder", 1, 0)]);
    }

    #[tokio::test]
    async fn rates_cover_only_the_period_asked_for() {
        let pool = with_a_sending().await;
        record_delivery(&pool, "abc", "newsletter", None)
            .await
            .expect("delivery");

        sqlx::query("UPDATE email_deliveries SET sent_on = date('now', '-10 days')")
            .execute(&pool)
            .await
            .expect("update");

        assert!(open_rates(&pool, 7).await.expect("rates").is_empty());
        assert_eq!(open_rates(&pool, 30).await.expect("rates").len(), 1);
    }
}
