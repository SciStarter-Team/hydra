//! Forgetting a deleted account (`common::user_deletions` says when).
//!
//! The caches of what it did go. The logs keep their rows of it, since they
//! are the record of what happened; what is built from them afresh (the
//! project stats snapshot) leaves the account out by [`forgotten`].

use std::collections::HashSet;

use anyhow::Result;
use common::{shared::db, transaction::Immediate};
use sqlx::SqlitePool;
use ulid::Ulid;

/// Forget the deleted account `user_id`.
pub async fn forget(user_id: Ulid) -> Result<()> {
    forget_in(db(), user_id).await
}

async fn forget_in(pool: &SqlitePool, user_id: Ulid) -> Result<()> {
    let user = user_id.to_string();
    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    sqlx::query!(
        r#"INSERT INTO "forgotten_users" ("user_id") VALUES (?) ON CONFLICT DO NOTHING"#,
        user
    )
    .execute(&mut tx)
    .await?;
    for clear in [
        r#"DELETE FROM "acts_of_science" WHERE "user_id" = ?"#,
        r#"DELETE FROM "articulate_completions_cache" WHERE "user_id" = ?"#,
        r#"DELETE FROM "project_participations_cache" WHERE "user_id" = ?"#,
        r#"DELETE FROM "project_ratings_cache" WHERE "user_id" = ?"#,
        r#"DELETE FROM "project_reviews_cache" WHERE "user_id" = ?"#,
        r#"DELETE FROM "project_stats_participants" WHERE "user_id" = ?"#,
        r#"DELETE FROM "project_stats_people" WHERE "user_id" = ?"#,
        r#"DELETE FROM "project_stats_viewers" WHERE "user_id" = ?"#,
        r#"DELETE FROM "user_emails_cache" WHERE "user_id" = ?"#,
        r#"DELETE FROM "user_tags_cache" WHERE "user_id" = ?"#,
        r#"DELETE FROM "zooniverse_matched_by_mapping" WHERE "user_id" = ?"#,
    ] {
        sqlx::query(clear).bind(&user).execute(&mut tx).await?;
    }

    tx.commit().await?;
    Ok(())
}

/// The accounts deleted.
pub async fn forgotten() -> Result<HashSet<Ulid>> {
    forgotten_in(db()).await
}

async fn forgotten_in(pool: &SqlitePool) -> Result<HashSet<Ulid>> {
    sqlx::query_scalar!(r#"SELECT "user_id" FROM "forgotten_users""#)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|user| Ok(user.parse()?))
        .collect()
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    #[tokio::test]
    async fn a_forgotten_account_loses_its_caches_and_is_remembered_as_forgotten() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();

        let ann = Ulid::from_parts(1, 1);
        let bob = Ulid::from_parts(2, 2);
        for user in [ann, bob] {
            sqlx::query(
                r#"INSERT INTO "user_tags_cache" ("user_id", "tag", "updated_at")
                   VALUES (?, 'teacher', '2026-11-23T00:00:00Z')"#,
            )
            .bind(user.to_string())
            .execute(&pool)
            .await
            .unwrap();
        }

        forget_in(&pool, ann).await.unwrap();
        forget_in(&pool, ann).await.unwrap();

        let tagged: Vec<String> = sqlx::query_scalar(r#"SELECT "user_id" FROM "user_tags_cache""#)
            .fetch_all(&pool)
            .await
            .unwrap();
        check!(tagged == [bob.to_string()]);
        check!(forgotten_in(&pool).await.unwrap() == HashSet::from([ann]));
    }
}
