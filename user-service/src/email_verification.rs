//! Confirming that an account's owner reads the mail sent to its address.
//!
//! An account that starts out unverified is mailed a link holding a one-time
//! code, and following the link marks the address -- and so the account --
//! verified. Until then the account is not sent ordinary email and its badges
//! are withheld, so every page shows it a banner offering to send the link
//! again.
//!
//! The same links add an address to an account at its owner's asking: the
//! address joins the account, confirmed, when the link mailed to it is
//! followed, and not before.
//!
//! This is orrery's `EmailVerificationCode` and `User.resend_verification`,
//! with the same limits.

use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::{
        email::{CheckSendabilityRequest, CheckSendabilityResponse},
        user::{
            BouncedEmail, EmailAddRequest, EmailAddRequested, EmailVerificationResend,
            ResendEmailVerification, VerifiedEmail, VerifyEmail,
        },
    },
    service::{TriggerEvent, log_activity, request, trigger_email_to_address},
    shared::{db, hash_identifier, hash_identifier_capfirst, hash_identifier_lower},
    timestring::TimeString,
    transaction::Immediate,
    ulid::AsUlid,
};
use sqlx::SqlitePool;
use ulid::Ulid;

/// How long a confirmation link stays good, as an SQLite date modifier.
const CODE_LIFETIME: &str = "-7 days";

/// How often the confirmation email may be sent again on request. The
/// banner's resend link is on every page an unconfirmed account sees, so
/// these bound how much mail one account can make us send. Both count the
/// codes already issued to the address, the one sent at signup included, so
/// they hold across instances and restarts.
const RESEND_INTERVAL: &str = "-1 minute";
const RESEND_WINDOW: &str = "-1 day";
const RESEND_WINDOW_LIMIT: i64 = 5;

/// What following a code's link does for the address it was mailed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodePurpose {
    /// Confirms an address the account already has.
    Confirm,
    /// Adds the address to the account, confirmed.
    Add,
}

impl CodePurpose {
    fn adds_address(self) -> bool {
        self == CodePurpose::Add
    }
}

/// Mint a code for an address, recording that it was issued.
async fn issue_code(
    pool: &SqlitePool,
    user_id: Ulid,
    email: &str,
    purpose: CodePurpose,
) -> Result<String, Error> {
    let code = crate::secret_code::generate(crate::secret_code::CODE_BYTES);
    let user_id = user_id.to_string();
    let issued = TimeString::now().to_string();
    let adds_address = purpose.adds_address();

    sqlx::query!(
        r#"insert into "email_verification_codes" ("code", "user_id", "email", "issued", "adds_address")
           values (?, ?, ?, ?, ?)"#,
        code,
        user_id,
        email,
        issued,
        adds_address,
    )
    .execute(pool)
    .await?;

    Ok(code)
}

/// Why another code was not issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// The address was sent one less than a minute ago.
    TooSoon,
    /// The address has been sent as many as it may be in a day.
    LimitReached,
}

/// Mint another code for an address unless it has been sent one too recently
/// or too often, in which case the refusal says which.
async fn reissue_code(
    pool: &SqlitePool,
    user_id: Ulid,
    email: &str,
    purpose: CodePurpose,
) -> Result<Result<String, Refusal>, Error> {
    let code = crate::secret_code::generate(crate::secret_code::CODE_BYTES);
    let user_id = user_id.to_string();
    let issued = TimeString::now().to_string();
    let adds_address = purpose.adds_address();

    // Holding the write lock from the first read is what stops two requests
    // that land together from both passing the checks.
    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    let just_now = sqlx::query_scalar!(
        r#"select count(*) from "email_verification_codes"
           where "email" = ? and datetime("issued") > datetime('now', ?)"#,
        email,
        RESEND_INTERVAL,
    )
    .fetch_one(&mut tx)
    .await?;

    if just_now > 0 {
        tx.rollback().await?;
        return Ok(Err(Refusal::TooSoon));
    }

    let in_window = sqlx::query_scalar!(
        r#"select count(*) from "email_verification_codes"
           where "email" = ? and datetime("issued") > datetime('now', ?)"#,
        email,
        RESEND_WINDOW,
    )
    .fetch_one(&mut tx)
    .await?;

    if in_window >= RESEND_WINDOW_LIMIT {
        tx.rollback().await?;
        return Ok(Err(Refusal::LimitReached));
    }

    sqlx::query!(
        r#"insert into "email_verification_codes" ("code", "user_id", "email", "issued", "adds_address")
           values (?, ?, ?, ?, ?)"#,
        code,
        user_id,
        email,
        issued,
        adds_address,
    )
    .execute(&mut tx)
    .await?;

    tx.commit().await?;

    Ok(Ok(code))
}

/// Redeem a code: the address it was mailed to becomes verified, and the
/// account with it, and an address the code was mailed to add joins the
/// account. None when the code is unknown or has expired, when it vouches
/// for an address the account no longer has, or when another account has
/// taken the address it was to add.
async fn redeem_code(pool: &SqlitePool, code: &str) -> Result<Option<VerifiedEmail>, Error> {
    // Swept here so that the table cannot grow without bound, and so that an
    // expired code is simply an unknown one below.
    sqlx::query!(
        r#"delete from "email_verification_codes"
           where datetime("issued") < datetime('now', ?)"#,
        CODE_LIFETIME,
    )
    .execute(pool)
    .await?;

    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    let row = sqlx::query!(
        r#"select "user_id", "email", "adds_address" as "adds_address: bool"
           from "email_verification_codes" where "code" = ?"#,
        code,
    )
    .fetch_optional(&mut tx)
    .await?;

    let Some(row) = row else {
        tx.rollback().await?;
        return Ok(None);
    };

    if row.adds_address {
        let hash = hash_identifier(&row.email);
        let hash_lower = hash_identifier_lower(&row.email);
        let hash_capfirst = hash_identifier_capfirst(&row.email);

        // Nothing is added when any spelling of the address is already
        // taken; when it is this account's own, the update below confirms it.
        sqlx::query!(
            r#"insert into "emails" ("user_id", "email", "verified", "hash", "hash_lower", "hash_capfirst")
               values (?, ?, 1, ?, ?, ?)
               on conflict do nothing"#,
            row.user_id,
            row.email,
            hash,
            hash_lower,
            hash_capfirst,
        )
        .execute(&mut tx)
        .await?;
    }

    let confirmed = sqlx::query!(
        r#"update "emails" set "verified" = 1 where "user_id" = ? and "email" = ?"#,
        row.user_id,
        row.email,
    )
    .execute(&mut tx)
    .await?
    .rows_affected();

    // Every code outstanding for the address goes, not only this one: once
    // the address is confirmed the other links have nothing left to do, and
    // if the account has dropped the address none of them ever will.
    sqlx::query!(
        r#"delete from "email_verification_codes" where "user_id" = ? and "email" = ?"#,
        row.user_id,
        row.email,
    )
    .execute(&mut tx)
    .await?;

    tx.commit().await?;

    if confirmed == 0 {
        return Ok(None);
    }

    let user_id = row
        .user_id
        .as_ulid()
        .map_err(|e| anyhow::anyhow!("Invalid user ID in database: {e}"))?;

    crate::users::update_user_verified_status_in(pool, user_id).await?;

    Ok(Some(VerifiedEmail {
        user_id,
        email: row.email,
    }))
}

/// Record that mail to an address bounced: it stops counting as confirmed,
/// and its account stops counting as verified unless another of its
/// addresses still is. Returns the account, when the address is on one.
async fn unverify_bounced(pool: &SqlitePool, email: &str) -> Result<Option<Ulid>, Error> {
    // Matched the way a login matches an address, since the bounce report
    // need not spell it with the capitals its owner used.
    let hash_lower = hash_identifier_lower(email);

    let row = sqlx::query!(
        r#"update "emails" set "verified" = 0 where "hash_lower" = ? returning "user_id""#,
        hash_lower,
    )
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let user_id = row
        .user_id
        .as_ulid()
        .map_err(|e| anyhow::anyhow!("Invalid user ID in database: {e}"))?;

    crate::users::update_user_verified_status_in(pool, user_id).await?;

    Ok(Some(user_id))
}

/// Mail an address the link that confirms it.
///
/// The email is the one staff bind to the `email-validation` trigger. It is
/// directed at the address rather than at the user, because a send addressed
/// to an unverified user is skipped.
async fn send_confirmation_email(user_id: Ulid, email: &str, code: &str) -> Result<(), Error> {
    let verify_url = format!("https://{}/verify?code={code}", env!("DOMAIN"));

    trigger_email_to_address(
        TriggerEvent::EmailValidation,
        user_id,
        email,
        Some(&[
            ("verify_url".to_string(), verify_url),
            // The code alone, for a template written the way orrery's is:
            // with the link spelled out around it.
            ("verify_code".to_string(), code.to_string()),
        ]),
    )
    .await
}

/// Ask a newly created account to confirm its address.
pub(crate) async fn send_to_new_account(user_id: Ulid, email: &str) -> Result<(), Error> {
    let code = issue_code(db(), user_id, email, CodePurpose::Confirm).await?;

    send_confirmation_email(user_id, email, &code).await
}

/// Send an account's confirmation email again, within the limits above.
pub async fn resend(req: &Request) -> Result<EmailVerificationResend, Error> {
    let resend: ResendEmailVerification = flexbuffers::from_slice(&req.message.payload)?;
    let user_id_str = resend.user_id.to_string();

    let email = sqlx::query_scalar!(
        r#"select "primary_email" from "users" where "user_id" = ?"#,
        user_id_str,
    )
    .fetch_optional(db())
    .await?
    .ok_or_else(|| anyhow::anyhow!("User not found"))?;

    // The emailer would drop the message without a word, and the banner would
    // go on promising an email that is never coming. Asked before a code is
    // issued, so that a refusal does not count against the limits.
    let sendability: CheckSendabilityResponse = request(
        "emailer.analytics.check_sendability",
        CheckSendabilityRequest {
            email_address: email.clone(),
        },
    )
    .await?;

    if !sendability.can_send {
        return Ok(EmailVerificationResend::Undeliverable);
    }

    let code = match reissue_code(db(), resend.user_id, &email, CodePurpose::Confirm).await? {
        Ok(code) => code,
        Err(Refusal::TooSoon) => return Ok(EmailVerificationResend::TooSoon),
        Err(Refusal::LimitReached) => return Ok(EmailVerificationResend::LimitReached),
    };

    send_confirmation_email(resend.user_id, &email, &code).await?;

    Ok(EmailVerificationResend::Sent)
}

/// Redeem the code from a confirmation email. None when the link is no
/// longer good.
pub async fn verify(req: &Request) -> Result<Option<VerifiedEmail>, Error> {
    let verify: VerifyEmail = flexbuffers::from_slice(&req.message.payload)?;

    let verified = redeem_code(db(), &verify.code).await?;

    if let Some(verified) = &verified {
        log_activity(None, "email_verified", verified.user_id, None, None)
            .await
            .ok();

        // The code may have added the address.
        crate::users::announce_user_emails(&verified.user_id.to_string()).await?;
    }

    Ok(verified)
}

/// Mail an address the link that adds it to the account, unless it is not
/// one mail can reach, someone already has it, or it has been sent too many.
pub async fn request_add(req: &Request) -> Result<EmailAddRequested, Error> {
    let add: EmailAddRequest = flexbuffers::from_slice(&req.message.payload)?;
    let email = add.email.trim();

    if !common::email::valid(email) {
        return Ok(EmailAddRequested::Invalid);
    }

    // Matched in every spelling that a partner's report of it would be, since
    // those are what the address would claim.
    match crate::user_matching::match_by_email(email).await? {
        Some(owner) if owner == add.user_id.to_string() => {
            return Ok(EmailAddRequested::AlreadyYours);
        }
        Some(_) => return Ok(EmailAddRequested::Taken),
        None => {}
    }

    let sendability: CheckSendabilityResponse = request(
        "emailer.analytics.check_sendability",
        CheckSendabilityRequest {
            email_address: email.to_string(),
        },
    )
    .await?;

    if !sendability.can_send {
        return Ok(EmailAddRequested::Undeliverable);
    }

    let code = match reissue_code(db(), add.user_id, email, CodePurpose::Add).await? {
        Ok(code) => code,
        Err(Refusal::TooSoon) => return Ok(EmailAddRequested::TooSoon),
        Err(Refusal::LimitReached) => return Ok(EmailAddRequested::LimitReached),
    };

    send_confirmation_email(add.user_id, email, &code).await?;

    log_activity(
        None,
        "email-add-requested",
        add.user_id,
        None,
        format!("email={email}"),
    )
    .await
    .ok();

    Ok(EmailAddRequested::Sent)
}

/// The addresses an account has asked to add whose links are still good.
pub(crate) async fn pending_additions(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<Vec<String>, Error> {
    Ok(sqlx::query_scalar!(
        r#"select distinct "email" from "email_verification_codes"
           where "user_id" = ? and "adds_address"
             and datetime("issued") >= datetime('now', ?)
           order by "email""#,
        user_id,
        CODE_LIFETIME,
    )
    .fetch_all(pool)
    .await?)
}

/// Take note that mail to an address has bounced.
pub async fn bounced(req: &Request) -> Result<(), Error> {
    let bounce: BouncedEmail = flexbuffers::from_slice(&req.message.payload)?;

    if let Some(user_id) = unverify_bounced(db(), &bounce.email).await? {
        println!("Address bounced, no longer counted as confirmed: {user_id}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use assert2::check;

    const ADA: &str = "01JGEDS64C0ECBWR472V7788BF";
    const BOB: &str = "01JGEDS64C0ECBWR472V7788BG";
    const ADDRESS: &str = "Ada@example.invalid";

    fn ada() -> Ulid {
        ADA.parse().unwrap()
    }

    /// The codes table as its migration makes it, beside as much of the
    /// accounts it refers to as verification reads and writes.
    async fn fixture() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");

        sqlx::raw_sql(
            r#"create table "users" (
                 "user_id" text primary key not null,
                 "verified" boolean not null
               );
               create table "emails" (
                 "user_id" text not null references "users" ("user_id") on delete cascade,
                 "email" text not null unique,
                 "verified" boolean not null,
                 "hash" text not null unique,
                 "hash_lower" text not null unique,
                 "hash_capfirst" text not null unique
               );"#,
        )
        .execute(&pool)
        .await
        .expect("accounts");

        for migration in [
            include_str!("../migrations/20261006154600_email_verification_codes.up.sql"),
            include_str!("../migrations/20261030230418_email_codes_that_add_addresses.up.sql"),
        ] {
            sqlx::raw_sql(migration)
                .execute(&pool)
                .await
                .expect("schema");
        }

        sqlx::query(r#"insert into "users" ("user_id", "verified") values (?, 0)"#)
            .bind(ADA)
            .execute(&pool)
            .await
            .expect("user");

        add_address(&pool, ADDRESS, false).await;

        pool
    }

    async fn add_address(pool: &SqlitePool, email: &str, verified: bool) {
        add_address_to(pool, ADA, email, verified).await;
    }

    async fn add_address_to(pool: &SqlitePool, user_id: &str, email: &str, verified: bool) {
        sqlx::query(
            r#"insert into "emails" ("user_id", "email", "verified", "hash", "hash_lower", "hash_capfirst")
               values (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(user_id)
        .bind(email)
        .bind(verified)
        .bind(hash_identifier(email))
        .bind(hash_identifier_lower(email))
        .bind(hash_identifier_capfirst(email))
        .execute(pool)
        .await
        .expect("address");
    }

    /// Move every code issued so far back in time, as though it had been
    /// issued that long ago.
    async fn age_codes(pool: &SqlitePool, modifier: &str) {
        sqlx::query(
            r#"update "email_verification_codes"
               set "issued" = strftime('%Y-%m-%dT%H:%M:%fZ', "issued", ?)"#,
        )
        .bind(modifier)
        .execute(pool)
        .await
        .expect("aged");
    }

    async fn user_verified(pool: &SqlitePool) -> bool {
        sqlx::query_scalar(r#"select "verified" from "users" where "user_id" = ?"#)
            .bind(ADA)
            .fetch_one(pool)
            .await
            .expect("user")
    }

    async fn address_verified(pool: &SqlitePool, email: &str) -> bool {
        sqlx::query_scalar(r#"select "verified" from "emails" where "email" = ?"#)
            .bind(email)
            .fetch_one(pool)
            .await
            .expect("address")
    }

    async fn outstanding(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar(r#"select count(*) from "email_verification_codes""#)
            .fetch_one(pool)
            .await
            .expect("count")
    }

    #[tokio::test]
    async fn redeeming_a_code_verifies_the_address_and_the_account() {
        let pool = fixture().await;
        let code = issue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
            .await
            .unwrap();

        let verified = redeem_code(&pool, &code).await.unwrap();

        assert_eq!(
            verified,
            Some(VerifiedEmail {
                user_id: ada(),
                email: ADDRESS.to_string()
            })
        );
        assert!(address_verified(&pool, ADDRESS).await);
        assert!(user_verified(&pool).await);
    }

    #[tokio::test]
    async fn a_code_is_good_once() {
        let pool = fixture().await;
        let code = issue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
            .await
            .unwrap();

        assert!(redeem_code(&pool, &code).await.unwrap().is_some());
        assert_eq!(redeem_code(&pool, &code).await.unwrap(), None);
    }

    /// The email is sent again when the first one is lost, so an account
    /// often holds several links, and whichever is followed settles it.
    #[tokio::test]
    async fn confirming_spends_every_code_sent_to_the_address() {
        let pool = fixture().await;
        let first = issue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
            .await
            .unwrap();
        let second = issue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
            .await
            .unwrap();

        assert!(redeem_code(&pool, &second).await.unwrap().is_some());

        assert_eq!(outstanding(&pool).await, 0);
        assert_eq!(redeem_code(&pool, &first).await.unwrap(), None);
    }

    #[tokio::test]
    async fn an_unknown_code_verifies_nothing() {
        let pool = fixture().await;
        issue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
            .await
            .unwrap();

        assert_eq!(redeem_code(&pool, "not-a-code").await.unwrap(), None);
        assert!(!user_verified(&pool).await);
        assert_eq!(outstanding(&pool).await, 1);
    }

    #[tokio::test]
    async fn a_code_expires_after_a_week() {
        let pool = fixture().await;
        let code = issue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
            .await
            .unwrap();

        age_codes(&pool, "-6 days").await;
        let fresh = issue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
            .await
            .unwrap();
        age_codes(&pool, "-2 days").await;

        // The first is now eight days old and the second two.
        assert_eq!(redeem_code(&pool, &code).await.unwrap(), None);
        assert!(!user_verified(&pool).await);
        assert_eq!(outstanding(&pool).await, 1, "the expired code is swept");

        assert!(redeem_code(&pool, &fresh).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_code_for_an_address_the_account_dropped_verifies_nothing() {
        let pool = fixture().await;
        let code = issue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
            .await
            .unwrap();

        sqlx::query(r#"delete from "emails""#)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(redeem_code(&pool, &code).await.unwrap(), None);
        assert!(!user_verified(&pool).await);
        assert_eq!(outstanding(&pool).await, 0);
    }

    #[tokio::test]
    async fn a_second_email_within_the_minute_is_refused() {
        let pool = fixture().await;
        issue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
            .await
            .unwrap();

        assert_eq!(
            reissue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
                .await
                .unwrap(),
            Err(Refusal::TooSoon)
        );
        assert_eq!(outstanding(&pool).await, 1);

        age_codes(&pool, "-61 seconds").await;

        assert!(
            reissue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
                .await
                .unwrap()
                .is_ok()
        );
        assert_eq!(outstanding(&pool).await, 2);
    }

    /// The email sent at signup counts toward the day's allowance.
    #[tokio::test]
    async fn the_daily_limit_counts_codes_already_issued() {
        let pool = fixture().await;

        issue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
            .await
            .unwrap();
        age_codes(&pool, "-2 minutes").await;

        for _ in 1..RESEND_WINDOW_LIMIT {
            assert!(
                reissue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
                    .await
                    .unwrap()
                    .is_ok()
            );
            age_codes(&pool, "-2 minutes").await;
        }

        assert_eq!(
            reissue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
                .await
                .unwrap(),
            Err(Refusal::LimitReached)
        );
        assert_eq!(outstanding(&pool).await, RESEND_WINDOW_LIMIT);
    }

    #[tokio::test]
    async fn the_daily_limit_forgets_codes_more_than_a_day_old() {
        let pool = fixture().await;

        for _ in 0..RESEND_WINDOW_LIMIT {
            issue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
                .await
                .unwrap();
        }
        age_codes(&pool, "-25 hours").await;

        assert!(
            reissue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
                .await
                .unwrap()
                .is_ok()
        );
    }

    /// The limits count what one address was sent, not what the table holds.
    #[tokio::test]
    async fn another_address_being_sent_one_does_not_hold_this_one_back() {
        let pool = fixture().await;
        add_address(&pool, "countess@example.invalid", false).await;

        issue_code(
            &pool,
            ada(),
            "countess@example.invalid",
            CodePurpose::Confirm,
        )
        .await
        .unwrap();

        assert!(
            reissue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
                .await
                .unwrap()
                .is_ok()
        );
    }

    #[tokio::test]
    async fn a_bounce_unverifies_the_address_however_it_is_spelled() {
        let pool = fixture().await;
        let code = issue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
            .await
            .unwrap();
        redeem_code(&pool, &code).await.unwrap();

        let account = unverify_bounced(&pool, &ADDRESS.to_lowercase())
            .await
            .unwrap();

        assert_eq!(account, Some(ada()));
        assert!(!address_verified(&pool, ADDRESS).await);
        assert!(!user_verified(&pool).await);
    }

    #[tokio::test]
    async fn an_account_with_another_confirmed_address_stays_verified() {
        let pool = fixture().await;
        let code = issue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
            .await
            .unwrap();
        redeem_code(&pool, &code).await.unwrap();
        add_address(&pool, "countess@example.invalid", true).await;

        unverify_bounced(&pool, ADDRESS).await.unwrap();

        assert!(!address_verified(&pool, ADDRESS).await);
        assert!(user_verified(&pool).await);
    }

    const SECOND: &str = "Countess@Example.invalid";

    #[tokio::test]
    async fn a_code_mailed_to_add_an_address_adds_it_confirmed() {
        let pool = fixture().await;
        let code = issue_code(&pool, ada(), SECOND, CodePurpose::Add)
            .await
            .unwrap();

        check!(pending_additions(&pool, ADA).await.unwrap() == vec![SECOND.to_string()]);

        let verified = redeem_code(&pool, &code).await.unwrap();

        check!(
            verified
                == Some(VerifiedEmail {
                    user_id: ada(),
                    email: SECOND.to_string()
                })
        );
        check!(address_verified(&pool, SECOND).await);
        check!(user_verified(&pool).await);
        check!(pending_additions(&pool, ADA).await.unwrap().is_empty());
    }

    /// Asked for, an address is not the account's until the link is followed.
    #[tokio::test]
    async fn an_address_waiting_to_be_added_is_not_on_the_account() {
        let pool = fixture().await;
        issue_code(&pool, ada(), SECOND, CodePurpose::Add)
            .await
            .unwrap();

        let on_account: i64 =
            sqlx::query_scalar(r#"select count(*) from "emails" where "email" = ?"#)
                .bind(SECOND)
                .fetch_one(&pool)
                .await
                .unwrap();

        check!(on_account == 0);
    }

    #[tokio::test]
    async fn an_address_another_account_took_meanwhile_is_not_added() {
        let pool = fixture().await;
        let code = issue_code(&pool, ada(), SECOND, CodePurpose::Add)
            .await
            .unwrap();

        sqlx::query(r#"insert into "users" ("user_id", "verified") values (?, 0)"#)
            .bind(BOB)
            .execute(&pool)
            .await
            .unwrap();
        add_address_to(&pool, BOB, &SECOND.to_lowercase(), false).await;

        check!(redeem_code(&pool, &code).await.unwrap() == None);
        check!(!user_verified(&pool).await);
        check!(outstanding(&pool).await == 0);
    }

    #[tokio::test]
    async fn a_link_to_add_an_address_already_added_confirms_it() {
        let pool = fixture().await;
        let code = issue_code(&pool, ada(), ADDRESS, CodePurpose::Add)
            .await
            .unwrap();

        check!(redeem_code(&pool, &code).await.unwrap().is_some());
        check!(address_verified(&pool, ADDRESS).await);
    }

    /// Codes that confirm an address the account has are not additions, and
    /// spent links are not waiting for anything.
    #[tokio::test]
    async fn only_live_additions_are_pending() {
        let pool = fixture().await;
        issue_code(&pool, ada(), ADDRESS, CodePurpose::Confirm)
            .await
            .unwrap();
        issue_code(&pool, ada(), SECOND, CodePurpose::Add)
            .await
            .unwrap();

        check!(pending_additions(&pool, ADA).await.unwrap() == vec![SECOND.to_string()]);

        age_codes(&pool, "-8 days").await;

        check!(pending_additions(&pool, ADA).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_bounce_from_an_address_nobody_has_is_ignored() {
        let pool = fixture().await;

        assert_eq!(
            unverify_bounced(&pool, "stranger@example.invalid")
                .await
                .unwrap(),
            None
        );
    }
}
