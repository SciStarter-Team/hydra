use crate::db;
use anyhow::Result;
use common::message::user::{
    ActivityAnalyticsRequest, ActivityAnalyticsResponse, ActivityStat, AdminAnalyticsResponse,
    GeographicAnalyticsRequest, GeographicAnalyticsResponse, GeographicStat, UserCountResponse,
};

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

    // Get monthly verified signups (last 30 days)
    let monthly_signups = sqlx::query!(
        "SELECT COUNT(*) as count FROM users
         WHERE date_joined > datetime('now', '-30 days')
         AND verified = 1"
    )
    .fetch_one(db())
    .await?
    .count as u32;

    // Get weekly verified signups (last 7 days)
    let weekly_signups = sqlx::query!(
        "SELECT COUNT(*) as count FROM users
         WHERE date_joined > datetime('now', '-7 days')
         AND verified = 1"
    )
    .fetch_one(db())
    .await?
    .count as u32;

    // Get daily verified signups (today's calendar date)
    let daily_signups = sqlx::query!(
        "SELECT COUNT(*) as count FROM users
         WHERE DATE(date_joined) = DATE('now')
         AND verified = 1"
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

/// Get geographic distribution of users
pub async fn geographic_analytics(
    req: &async_nats::service::Request,
) -> Result<GeographicAnalyticsResponse> {
    let request: GeographicAnalyticsRequest = flexbuffers::from_slice(&req.message.payload)?;

    // Get verified user count by country for the specified date range
    let stats_result = sqlx::query!(
        r#"
        SELECT
            COALESCE(country, 'Unknown') as country,
            COALESCE(country_code, 'XX') as country_code,
            COUNT(*) as user_count
        FROM users
        WHERE date(date_joined) >= ? AND date(date_joined) <= ?
        AND verified = 1
        GROUP BY country, country_code
        ORDER BY user_count DESC
        LIMIT 50
        "#,
        request.start_date,
        request.end_date
    )
    .fetch_all(db())
    .await?;

    let stats: Vec<GeographicStat> = stats_result
        .iter()
        .map(|row| GeographicStat {
            country: row.country.clone(),
            country_code: row.country_code.clone(),
            user_count: row.user_count,
        })
        .collect();

    Ok(GeographicAnalyticsResponse { stats })
}

/// Get user activity trends over time
pub async fn activity_analytics(
    req: &async_nats::service::Request,
) -> Result<ActivityAnalyticsResponse> {
    let request: ActivityAnalyticsRequest = flexbuffers::from_slice(&req.message.payload)?;

    // Get daily new user counts for the specified date range (all days for total count)
    let all_stats_result = sqlx::query!(
        r#"
        SELECT
            DATE(date_joined) as date,
            COUNT(*) as new_users,
            SUM(CASE WHEN verified = 1 THEN 1 ELSE 0 END) as new_verified
        FROM users
        WHERE date(date_joined) >= ? AND date(date_joined) <= ?
        GROUP BY DATE(date_joined)
        ORDER BY date DESC
        "#,
        request.start_date,
        request.end_date
    )
    .fetch_all(db())
    .await?;

    let total_count = all_stats_result.len();

    // Get total users and verified users up to and including the end date for cumulative calculation
    let end_count_result = sqlx::query!(
        r#"
        SELECT
            COUNT(*) as count,
            SUM(CASE WHEN verified = 1 THEN 1 ELSE 0 END) as verified_count
        FROM users
        WHERE date(date_joined) <= ?
        "#,
        request.end_date
    )
    .fetch_one(db())
    .await?;

    // Calculate cumulative totals and paginate (working backwards from end date)
    let offset = ((request.page - 1) * request.per_page) as usize;
    let limit = request.per_page as usize;

    let mut cumulative_total = end_count_result.count;
    let mut cumulative_verified = end_count_result.verified_count.unwrap_or(0);

    // Subtract users from rows before the offset (going backwards)
    for row in all_stats_result.iter().take(offset) {
        cumulative_total -= row.new_users;
        cumulative_verified -= row.new_verified;
    }

    // Get paginated results with correct cumulative totals (decreasing as we go back in time)
    let stats: Vec<ActivityStat> = all_stats_result
        .iter()
        .skip(offset)
        .take(limit)
        .map(|row| {
            let date_cumulative = cumulative_total;
            let date_verified = cumulative_verified;
            cumulative_total -= row.new_users;
            cumulative_verified -= row.new_verified;
            ActivityStat {
                date: row.date.clone().unwrap_or_else(|| "Unknown".to_string()),
                new_users: row.new_verified,
                total_users_to_date: date_cumulative,
                verified_users_to_date: date_verified,
            }
        })
        .collect();

    Ok(ActivityAnalyticsResponse { stats, total_count })
}
