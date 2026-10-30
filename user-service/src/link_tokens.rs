//! Link Tokens, which an account holder gives an affiliate project so that it
//! can report the account's contributions (`/api/participation/link-token`).
//!
//! A token is a ULID string, as on orrery, and orrery's tokens are copied here
//! with their strings unchanged, so a token issued by either works here.

use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::dashboard::{
        DEFAULT_LINK_TOKEN_LABEL, LINK_TOKEN_LABEL_LENGTH, LinkToken, LinkTokenAdd, LinkTokenAdded,
        LinkTokenDelete,
    },
    service::decode_structured_message,
    shared::db,
    timestring::TimeString,
    transaction::Immediate,
};
use jiff::Timestamp;
use sqlx::SqlitePool;
use ulid::Ulid;

pub async fn list_by_user(req: &Request) -> Result<Vec<LinkToken>, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;
    list(db(), &user_id).await
}

async fn list(pool: &SqlitePool, user_id: &str) -> Result<Vec<LinkToken>, Error> {
    let rows = sqlx::query!(
        r#"SELECT "token", "label", "created"
        FROM "link_tokens"
        WHERE "user_id" = ?
        ORDER BY "created", "token""#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| LinkToken {
            token: row.token,
            label: row.label,
            created: row.created,
        })
        .collect())
}

pub async fn add(req: &Request) -> Result<LinkTokenAdded, Error> {
    let add: LinkTokenAdd = decode_structured_message(req.message.clone())?;
    add_token(db(), add.user_id, &add.label).await
}

/// Mint a token for `user_id`, labelled `label` once trimmed, or "default" if
/// that leaves nothing. An account's labels are distinct.
async fn add_token(pool: &SqlitePool, user_id: Ulid, label: &str) -> Result<LinkTokenAdded, Error> {
    let label = match label.trim() {
        "" => DEFAULT_LINK_TOKEN_LABEL,
        label => label,
    };

    if label.chars().count() > LINK_TOKEN_LABEL_LENGTH {
        return Ok(LinkTokenAdded::LabelTooLong);
    }

    let user_id = user_id.to_string();
    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    let taken = sqlx::query_scalar!(
        r#"SELECT EXISTS (
             SELECT 1 FROM "link_tokens" WHERE "user_id" = ? AND "label" = ?
           ) AS "taken: bool""#,
        user_id,
        label,
    )
    .fetch_one(&mut tx)
    .await?;

    if taken {
        return Ok(LinkTokenAdded::LabelTaken(label.to_string()));
    }

    let token = Ulid::new();
    let created = minted_at(&token).to_string();
    let token = token.to_string();

    sqlx::query!(
        r#"INSERT INTO "link_tokens" ("token", "user_id", "label", "created")
        VALUES (?, ?, ?, ?)"#,
        token,
        user_id,
        label,
        created,
    )
    .execute(&mut tx)
    .await?;

    tx.commit().await?;

    Ok(LinkTokenAdded::Added(LinkToken {
        token,
        label: label.to_string(),
        created,
    }))
}

pub async fn delete(req: &Request) -> Result<(), Error> {
    let delete: LinkTokenDelete = decode_structured_message(req.message.clone())?;
    let user_id_str = delete.user_id.to_string();

    sqlx::query!(
        r#"DELETE FROM "link_tokens" WHERE "token" = ? AND "user_id" = ?"#,
        delete.token,
        user_id_str,
    )
    .execute(db())
    .await?;

    Ok(())
}

/// When a token was made, which its ULID records. Neither side stores it
/// otherwise: orrery keeps no creation time on a token at all.
fn minted_at(token: &Ulid) -> TimeString {
    Timestamp::try_from(token.datetime())
        .map(TimeString::from)
        .unwrap_or_else(|_| TimeString::now())
}

/// One of orrery's Link Tokens, as `/staff/sync/link_tokens` lists it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OrreryLinkToken {
    pub id: i64,
    pub user_ulid: String,
    pub name: String,
    pub token: String,
}

/// Make the tokens copied from orrery match `tokens`, the whole of orrery's
/// set, in one transaction: each is added or brought up to date, and a copy
/// whose original is gone is deleted. Tokens made here are left alone.
///
/// A token whose account has not synced yet is skipped; the next snapshot
/// brings it. Its creation time is read from its ULID, and failing that is
/// when it was first copied.
pub async fn mirror_orrery(tokens: &[OrreryLinkToken]) -> Result<(), Error> {
    mirror(db(), tokens).await
}

async fn mirror(pool: &SqlitePool, tokens: &[OrreryLinkToken]) -> Result<(), Error> {
    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    for t in tokens {
        let created = Ulid::from_string(&t.token)
            .map(|token| minted_at(&token))
            .unwrap_or_else(|_| TimeString::now())
            .to_string();

        sqlx::query!(
            r#"INSERT INTO "link_tokens" ("token", "user_id", "label", "created", "orrery_id")
               SELECT ?, ?, ?, ?, ?
               WHERE EXISTS (SELECT 1 FROM "users" WHERE "user_id" = ?)
               ON CONFLICT ("token") DO UPDATE SET
                 "user_id" = excluded."user_id",
                 "label" = excluded."label",
                 "orrery_id" = excluded."orrery_id""#,
            t.token,
            t.user_ulid,
            t.name,
            created,
            t.id,
            t.user_ulid,
        )
        .execute(&mut tx)
        .await?;
    }

    let ids = serde_json::to_string(&tokens.iter().map(|t| t.id).collect::<Vec<_>>())?;

    sqlx::query!(
        r#"DELETE FROM "link_tokens"
           WHERE "orrery_id" IS NOT NULL
             AND "orrery_id" NOT IN (SELECT "value" FROM json_each(?))"#,
        ids,
    )
    .execute(&mut tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    const ADA: &str = "01JADA0000000000000000000A";
    const GRACE: &str = "01JGRACE000000000000000000";

    /// The tokens table as its migrations make it, beside the accounts it
    /// refers to.
    async fn fixture() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");

        sqlx::raw_sql(r#"create table "users" ("user_id" text primary key not null);"#)
            .execute(&pool)
            .await
            .expect("accounts");

        for migration in [
            include_str!("../migrations/20251113163913_link_tokens.up.sql"),
            include_str!("../migrations/20261030204526_link_tokens_from_orrery.up.sql"),
        ] {
            sqlx::raw_sql(migration)
                .execute(&pool)
                .await
                .expect("schema");
        }

        for user_id in [ADA, GRACE] {
            sqlx::query(r#"insert into "users" ("user_id") values (?)"#)
                .bind(user_id)
                .execute(&pool)
                .await
                .expect("user");
        }

        pool
    }

    fn ada() -> Ulid {
        ADA.parse().unwrap()
    }

    fn orrery(id: i64, user_ulid: &str, name: &str, token: &str) -> OrreryLinkToken {
        OrreryLinkToken {
            id,
            user_ulid: user_ulid.to_string(),
            name: name.to_string(),
            token: token.to_string(),
        }
    }

    async fn labels(pool: &SqlitePool, user_id: &str) -> Vec<String> {
        list(pool, user_id)
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.label)
            .collect()
    }

    #[tokio::test]
    async fn a_new_token_is_a_ulid_like_orrerys() {
        let pool = fixture().await;

        let LinkTokenAdded::Added(token) = add_token(&pool, ada(), "Stall Catchers").await.unwrap()
        else {
            panic!("not added");
        };

        let ulid = Ulid::from_string(&token.token).unwrap();
        check!(token.token == ulid.to_string());
        check!(token.token.len() == 26);
        check!(token.created == minted_at(&ulid).to_string());
    }

    #[tokio::test]
    async fn a_blank_label_is_default() {
        let pool = fixture().await;

        add_token(&pool, ada(), "   ").await.unwrap();

        check!(labels(&pool, ADA).await == ["default"]);
    }

    #[tokio::test]
    async fn a_label_is_trimmed_and_used_once_per_account() {
        let pool = fixture().await;

        add_token(&pool, ada(), "Stall Catchers").await.unwrap();
        let again = add_token(&pool, ada(), "  Stall Catchers ").await.unwrap();
        add_token(&pool, GRACE.parse().unwrap(), "Stall Catchers")
            .await
            .unwrap();

        check!(matches!(again, LinkTokenAdded::LabelTaken(label) if label == "Stall Catchers"));
        check!(labels(&pool, ADA).await == ["Stall Catchers"]);
        check!(labels(&pool, GRACE).await == ["Stall Catchers"]);
    }

    #[tokio::test]
    async fn a_label_longer_than_orrery_holds_is_refused() {
        let pool = fixture().await;

        let fits = add_token(&pool, ada(), &"é".repeat(LINK_TOKEN_LABEL_LENGTH))
            .await
            .unwrap();
        let too_long = add_token(&pool, ada(), &"x".repeat(LINK_TOKEN_LABEL_LENGTH + 1))
            .await
            .unwrap();

        check!(matches!(fits, LinkTokenAdded::Added(_)));
        check!(matches!(too_long, LinkTokenAdded::LabelTooLong));
    }

    #[tokio::test]
    async fn orrerys_tokens_are_copied_with_their_strings() {
        let pool = fixture().await;
        let token = "01HQ3Z8Y6V2N4J5K7M8P9R0S1T";

        mirror(&pool, &[orrery(7, ADA, "default", token)])
            .await
            .unwrap();

        let copied = list(&pool, ADA).await.unwrap();
        check!(copied.len() == 1);
        check!(copied[0].token == token);
        check!(copied[0].label == "default");
        check!(copied[0].created == minted_at(&Ulid::from_string(token).unwrap()).to_string());
    }

    #[tokio::test]
    async fn a_token_deleted_on_orrery_goes_and_one_made_here_stays() {
        let pool = fixture().await;
        let LinkTokenAdded::Added(native) = add_token(&pool, ada(), "here").await.unwrap() else {
            panic!("not added");
        };

        mirror(
            &pool,
            &[
                orrery(1, ADA, "first", "01HQ3Z8Y6V2N4J5K7M8P9R0S1A"),
                orrery(2, ADA, "second", "01HQ3Z8Y6V2N4J5K7M8P9R0S1B"),
            ],
        )
        .await
        .unwrap();
        mirror(
            &pool,
            &[orrery(2, ADA, "second", "01HQ3Z8Y6V2N4J5K7M8P9R0S1B")],
        )
        .await
        .unwrap();

        let tokens: Vec<String> = list(&pool, ADA)
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.token)
            .collect();
        check!(tokens.len() == 2);
        check!(tokens.contains(&"01HQ3Z8Y6V2N4J5K7M8P9R0S1B".to_string()));
        check!(tokens.contains(&native.token));
    }

    #[tokio::test]
    async fn a_token_for_an_account_not_yet_synced_waits() {
        let pool = fixture().await;
        let stranger = "01JSTRANGER000000000000000";

        mirror(
            &pool,
            &[orrery(1, stranger, "default", "01HQ3Z8Y6V2N4J5K7M8P9R0S1A")],
        )
        .await
        .unwrap();
        check!(labels(&pool, stranger).await.is_empty());

        sqlx::query(r#"insert into "users" ("user_id") values (?)"#)
            .bind(stranger)
            .execute(&pool)
            .await
            .unwrap();
        mirror(
            &pool,
            &[orrery(1, stranger, "default", "01HQ3Z8Y6V2N4J5K7M8P9R0S1A")],
        )
        .await
        .unwrap();
        check!(labels(&pool, stranger).await == ["default"]);
    }

    #[tokio::test]
    async fn an_empty_snapshot_removes_every_copy() {
        let pool = fixture().await;

        mirror(
            &pool,
            &[orrery(1, ADA, "default", "01HQ3Z8Y6V2N4J5K7M8P9R0S1A")],
        )
        .await
        .unwrap();
        mirror(&pool, &[]).await.unwrap();

        check!(labels(&pool, ADA).await.is_empty());
    }
}
