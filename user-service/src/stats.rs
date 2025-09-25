use crate::db;
use anyhow::Result;
use common::message::user::{AdminAnalyticsResponse, UserCountResponse};

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

/// Get analytics data for admin dashboard
pub async fn admin_analytics(
    _req: &async_nats::service::Request,
) -> Result<AdminAnalyticsResponse> {
    // Get total users
    let total_users = sqlx::query!("SELECT COUNT(*) as count FROM users")
        .fetch_one(db())
        .await?
        .count as u32;

    // Get verified users
    let verified_users = sqlx::query!("SELECT COUNT(*) as count FROM users WHERE verified = 1")
        .fetch_one(db())
        .await?
        .count as u32;

    // Get monthly signups (last 30 days)
    let monthly_signups = sqlx::query!(
        "SELECT COUNT(*) as count FROM users 
         WHERE date_joined > datetime('now', '-30 days')"
    )
    .fetch_one(db())
    .await?
    .count as u32;

    // Get weekly signups (last 7 days)
    let weekly_signups = sqlx::query!(
        "SELECT COUNT(*) as count FROM users 
         WHERE date_joined > datetime('now', '-7 days')"
    )
    .fetch_one(db())
    .await?
    .count as u32;

    // Get daily signups (last 24 hours)
    let daily_signups = sqlx::query!(
        "SELECT COUNT(*) as count FROM users 
         WHERE date_joined > datetime('now', '-1 day')"
    )
    .fetch_one(db())
    .await?
    .count as u32;

    Ok(AdminAnalyticsResponse {
        total_users,
        verified_users,
        monthly_signups,
        weekly_signups,
        daily_signups,
    })
}
