use crate::db;
use anyhow::Result;
use common::message::user::UserCountResponse;

/// Get total user count
pub async fn total_count(_req: &async_nats::service::Request) -> Result<UserCountResponse> {
    // Count total users from the database
    let user_count = sqlx::query!("SELECT COUNT(*) as count FROM users")
        .fetch_one(db())
        .await?
        .count as u64;

    Ok(UserCountResponse { count: user_count })
}

/// Get verified user count
pub async fn verified_count(_req: &async_nats::service::Request) -> Result<UserCountResponse> {
    // Count verified users from the database
    let user_count = sqlx::query!("SELECT COUNT(*) as count FROM users where verified = 1")
        .fetch_one(db())
        .await?
        .count as u64;

    Ok(UserCountResponse { count: user_count })
}
