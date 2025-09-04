use crate::error::{LogServiceError, Result};
use common::badge::Badge;
use common::message::log::{
    AnalyticsQuery, AnalyticsResponse, RecentActivityItem, RecentActivityRequest,
    RecentActivityResponse, WeeklyCount,
};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::functions::expr_fn::date_trunc;
use datafusion::functions_aggregate::expr_fn::count;
use datafusion::prelude::*;
use std::str::FromStr;

use crate::database::storage::Database;
use std::sync::Arc;

/// Get clickthrough analytics using DataFusion with safe parameter binding on pre-computed summary tables
pub async fn get_clickthrough_analytics(
    database: Arc<Database>,
    query: AnalyticsQuery,
) -> Result<AnalyticsResponse> {
    let ctx = database.context();

    // Use safe parameter binding with DataFusion expressions
    let df = ctx
        .table("clickthrough_weekly_summary")
        .await?
        .filter(col("week_start").gt_eq(lit(&query.start_date)))?
        .filter(col("week_start").lt_eq(lit(&query.end_date)))?
        .select(vec![col("week_start"), col("count")])?
        .sort(vec![col("week_start").sort(true, true)])?;

    let batches = df.collect().await?;
    let weekly_counts = extract_weekly_counts_from_batches(batches)?;

    Ok(AnalyticsResponse::WeeklyCounts(weekly_counts))
}

/// Get join analytics using DataFusion with safe parameter binding on pre-computed summary tables
pub async fn get_join_analytics(
    database: Arc<Database>,
    query: AnalyticsQuery,
) -> Result<AnalyticsResponse> {
    let ctx = database.context();

    // Use safe parameter binding with DataFusion expressions
    let df = ctx
        .table("join_weekly_summary")
        .await?
        .filter(col("week_start").gt_eq(lit(&query.start_date)))?
        .filter(col("week_start").lt_eq(lit(&query.end_date)))?
        .select(vec![col("week_start"), col("count")])?
        .sort(vec![col("week_start").sort(true, true)])?;

    let batches = df.collect().await?;
    let weekly_counts = extract_weekly_counts_from_batches(batches)?;

    Ok(AnalyticsResponse::WeeklyCounts(weekly_counts))
}

/// Helper function to extract WeeklyCount data from Arrow RecordBatches
fn extract_weekly_counts_from_batches(batches: Vec<RecordBatch>) -> Result<Vec<WeeklyCount>> {
    use datafusion::arrow::array::{Array, Date32Array, Int64Array, StringArray};
    let mut weekly_counts = Vec::new();

    for batch in batches {
        if batch.num_rows() == 0 {
            continue;
        }

        let week_start_array = batch.column(0);
        let count_array = batch.column(1);

        // Convert Arrow arrays to WeeklyCount structs
        for i in 0..batch.num_rows() {
            let week_start = if let Some(string_array) =
                week_start_array.as_any().downcast_ref::<StringArray>()
            {
                string_array.value(i).to_string()
            } else if let Some(date_array) = week_start_array.as_any().downcast_ref::<Date32Array>()
            {
                // Convert Date32 to string format (YYYY-MM-DD) using jiff
                let days_since_epoch = date_array.value(i);
                // Date32 is days since Unix epoch (1970-01-01)
                let unix_epoch = jiff::civil::Date::constant(1970, 1, 1);
                let date = unix_epoch
                    .checked_add(jiff::Span::new().days(days_since_epoch as i64))
                    .map_err(|_| {
                        LogServiceError::Analytics(format!(
                            "Invalid date value: {}",
                            days_since_epoch
                        ))
                    })?;
                date.to_string()
            } else {
                return Err(LogServiceError::UnsupportedDataType(format!(
                    "{:?}",
                    week_start_array.data_type()
                )));
            };

            let count = count_array
                .as_any()
                .downcast_ref::<Int64Array>()
                .ok_or_else(|| {
                    LogServiceError::Analytics(
                        "Failed to cast count column to Int64Array".to_string(),
                    )
                })?
                .value(i);

            weekly_counts.push(WeeklyCount { week_start, count });
        }
    }

    Ok(weekly_counts)
}

/// Generate clickthrough analytics for real-time data directly from main tables
/// This can be used for more detailed queries when summary tables are not sufficient
pub async fn get_clickthrough_analytics_realtime(
    database: Arc<Database>,
    query: AnalyticsQuery,
) -> Result<AnalyticsResponse> {
    let ctx = database.context();

    // Use DataFrame API for complex aggregations with safe parameter binding
    let df = ctx
        .table("clickthroughs")
        .await?
        .filter(col("when").gt_eq(lit(&query.start_date)))?
        .filter(col("when").lt_eq(lit(&query.end_date)))?
        .aggregate(
            vec![date_trunc(lit("week"), col("when")).alias("week_start")],
            vec![count(lit(1)).alias("count")],
        )?
        .sort(vec![col("week_start").sort(true, true)])?;

    let batches = df.collect().await?;
    let weekly_counts = extract_weekly_counts_from_batches(batches)?;

    Ok(AnalyticsResponse::WeeklyCounts(weekly_counts))
}

/// Generate join analytics for real-time data directly from main tables
pub async fn get_join_analytics_realtime(
    database: Arc<Database>,
    query: AnalyticsQuery,
) -> Result<AnalyticsResponse> {
    let ctx = database.context();

    // Use DataFrame API for complex aggregations with safe parameter binding
    let df = ctx
        .table("join_table")
        .await?
        .filter(col("when").gt_eq(lit(&query.start_date)))?
        .filter(col("when").lt_eq(lit(&query.end_date)))?
        .aggregate(
            vec![date_trunc(lit("week"), col("when")).alias("week_start")],
            vec![count(lit(1)).alias("count")],
        )?
        .sort(vec![col("week_start").sort(true, true)])?;

    let batches = df.collect().await?;
    let weekly_counts = extract_weekly_counts_from_batches(batches)?;

    Ok(AnalyticsResponse::WeeklyCounts(weekly_counts))
}

/// Get recent activity from the activity log
pub async fn get_recent_activity(
    database: Arc<Database>,
    request: RecentActivityRequest,
) -> Result<RecentActivityResponse> {
    let ctx = database.context();
    let limit = request.limit.unwrap_or(10) * 3; // Get more records to filter out uninteresting ones

    // Query multiple tables to get comprehensive recent activity (including WAL data)
    // Get activities from activity_log_realtime table (includes both parquet and WAL)
    let activity_df = ctx
        .table("activity_log_realtime")
        .await?
        .select(vec![
            col("when"),
            col("action"),
            col("user_id"),
            col("target"),
            col("extra"),
        ])?
        .filter(
            // Filter out system/bulk operations that aren't interesting for admin dashboard
            col("action")
                .not_eq(lit("imported"))
                .and(col("action").not_eq(lit("synced")))
                .and(col("action").not_eq(lit("processed")))
                .and(col("action").not_eq(lit("cached"))),
        )?;

    // Get page views and convert to activity format (includes WAL via union table)
    let pageview_df = ctx.table("page_views_realtime").await?.select(vec![
        col("when"),
        lit("pageview").alias("action"),
        col("user_id"),
        col("path").alias("target"),
        col("query").alias("extra"),
    ])?;

    // Get clickthroughs and convert to activity format (includes WAL val union table)
    let clickthrough_df = ctx.table("clickthroughs_realtime").await?.select(vec![
        col("when"),
        lit("clickthrough").alias("action"),
        col("user_id"),
        col("to_url").alias("target"),
        col("program_id").alias("extra"),
    ])?;

    // 3. Union activities and page views together
    let df = activity_df
        .union(pageview_df)?
        .union(clickthrough_df)?
        .sort(vec![col("when").sort(false, true)])? // DESC order (most recent first)
        .limit(0, Some(limit as usize))?;

    let batches = df.collect().await?;
    let mut activities = extract_activity_items_from_batches(batches)?;

    // Enhance activities with user emails and better action titles
    enhance_activities(database, &mut activities).await?;

    // Filter out activities that don't format well and limit to requested count
    activities.retain(|activity| {
        !activity.formatted_description.starts_with("imported:")
            && !activity.formatted_description.contains(": unknown")
    });
    activities.truncate(request.limit.unwrap_or(10) as usize);

    Ok(RecentActivityResponse { activities })
}

/// Helper function to extract RecentActivityItem data from Arrow RecordBatches
fn extract_activity_items_from_batches(
    batches: Vec<RecordBatch>,
) -> Result<Vec<RecentActivityItem>> {
    use datafusion::arrow::array::{Array, StringArray};
    let mut activities = Vec::new();

    for batch in batches {
        if batch.num_rows() == 0 {
            continue;
        }

        let when_array = batch.column(0);
        let action_array = batch.column(1);
        let user_id_array = batch.column(2);
        let target_array = batch.column(3);
        let extra_array = batch.column(4);

        for i in 0..batch.num_rows() {
            let when = when_array
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| {
                    LogServiceError::Analytics("Failed to cast when column".to_string())
                })?
                .value(i)
                .to_string();

            let action = action_array
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| {
                    LogServiceError::Analytics("Failed to cast action column".to_string())
                })?
                .value(i)
                .to_string();

            let user_id = if user_id_array.is_null(i) {
                None
            } else {
                Some(
                    user_id_array
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .ok_or_else(|| {
                            LogServiceError::Analytics("Failed to cast user_id column".to_string())
                        })?
                        .value(i)
                        .to_string(),
                )
            };

            let target = if target_array.is_null(i) {
                None
            } else {
                Some(
                    target_array
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .ok_or_else(|| {
                            LogServiceError::Analytics("Failed to cast target column".to_string())
                        })?
                        .value(i)
                        .to_string(),
                )
            };

            let extra = if extra_array.is_null(i) {
                None
            } else {
                Some(
                    extra_array
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .ok_or_else(|| {
                            LogServiceError::Analytics("Failed to cast extra column".to_string())
                        })?
                        .value(i)
                        .to_string(),
                )
            };

            // Format the activity for display
            let (formatted_description, icon) = format_activity(&action, &user_id, &target, &extra);
            let time_ago = format_time_ago(&when);

            activities.push(RecentActivityItem {
                when,
                action,
                user_id,
                target,
                extra,
                formatted_description,
                time_ago,
                icon,
            });
        }
    }

    Ok(activities)
}

/// Enhance activities with better user display names and action titles
async fn enhance_activities(
    _database: Arc<Database>,
    activities: &mut [RecentActivityItem],
) -> Result<()> {
    for activity in activities {
        // Replace ULID user_id with email if available
        if let Some(user_id) = &activity.user_id {
            if let Ok(email) = lookup_user_email_from_cache(user_id).await {
                activity.user_id = Some(email);
            }
        }

        // Convert action to human-readable title
        activity.action = humanize_action(&activity.action);

        // Re-format the description with enhanced data
        let (new_description, new_icon) = format_activity(
            &activity.action.to_lowercase().replace(" ", "_"), // Convert back for matching
            &activity.user_id,
            &activity.target,
            &activity.extra,
        );
        activity.formatted_description = new_description;
        activity.icon = new_icon;
    }

    Ok(())
}

/// Look up user email from the log service cache
async fn lookup_user_email_from_cache(user_id: &str) -> Result<String> {
    use common::shared::db;

    // Query the user_emails_cache table in the log service SQLite database
    let result = sqlx::query!(
        r#"SELECT "email" FROM "user_emails_cache" WHERE "user_id" = ? LIMIT 1"#,
        user_id
    )
    .fetch_optional(db())
    .await?;

    Ok(result
        .map(|row| row.email)
        .unwrap_or_else(|| user_id.to_string()))
}

/// Convert action names to human-readable titles
fn humanize_action(action: &str) -> String {
    match action {
        // We can add special cases here if needed
        _ => {
            // Convert snake_case and dot.case to Title Case
            action
                .replace("_", " ")
                .replace(".", " ")
                .split_whitespace()
                .map(|word| {
                    let mut chars = word.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
    }
}

/// Format activity for human-readable display
fn format_activity(
    action: &str,
    user_id: &Option<String>,
    target: &Option<String>,
    extra: &Option<String>,
) -> (String, String) {
    match action {
        "user_signup" => (
            format!(
                "New user registered: {}",
                user_id.as_deref().unwrap_or("someone")
            ),
            "user-plus".to_string(),
        ),
        "user_login" => (
            format!(
                "User logged in: {}",
                user_id.as_deref().unwrap_or("someone")
            ),
            "log-in".to_string(),
        ),
        "project_created" => (
            format!(
                "Created project: {}",
                target.as_deref().unwrap_or("Untitled Project")
            ),
            "folder-plus".to_string(),
        ),
        "event_created" => (
            format!(
                "Created event: {}",
                target.as_deref().unwrap_or("Untitled Event")
            ),
            "calendar-plus".to_string(),
        ),
        "badge_earned" => {
            let badge_name = extra
                .as_deref()
                .and_then(Badge::from_badgeclass)
                .map(|badge| badge.name())
                .unwrap_or("Unknown Badge");

            (
                format!(
                    "{} earned badge: {}",
                    user_id.as_deref().unwrap_or("someone"),
                    badge_name
                ),
                "award".to_string(),
            )
        }
        "topic_created" => (
            format!(
                "New forum topic created: {}",
                target.as_deref().unwrap_or("Untitled Topic")
            ),
            "message-square".to_string(),
        ),
        "topic_updated" => (
            format!(
                "Forum topic updated: {}",
                target.as_deref().unwrap_or("a topic")
            ),
            "edit".to_string(),
        ),
        "post_created" => (
            format!(
                "New forum post by {}",
                user_id.as_deref().unwrap_or("someone")
            ),
            "message-circle".to_string(),
        ),
        "post_updated" => (
            format!(
                "Forum post updated by {}",
                user_id.as_deref().unwrap_or("someone")
            ),
            "edit".to_string(),
        ),
        "moderation_action" => (
            format!(
                "Moderation action taken by {}",
                user_id.as_deref().unwrap_or("a moderator")
            ),
            "shield".to_string(),
        ),
        "forum_created" => (
            format!(
                "New forum created: {}",
                target.as_deref().unwrap_or("Untitled Forum")
            ),
            "users".to_string(),
        ),
        "forum_updated" => (
            format!("Forum updated: {}", target.as_deref().unwrap_or("a forum")),
            "edit".to_string(),
        ),
        "forum_moderator_added" => (
            format!(
                "New moderator added to forum: {}",
                user_id.as_deref().unwrap_or("someone")
            ),
            "user-plus".to_string(),
        ),
        "forum_moderator_removed" => (
            format!(
                "Moderator removed from forum: {}",
                user_id.as_deref().unwrap_or("someone")
            ),
            "user-minus".to_string(),
        ),
        "forum_category_created" => (
            format!(
                "New forum category created: {}",
                target.as_deref().unwrap_or("Untitled Category")
            ),
            "folder-plus".to_string(),
        ),
        "forum_category_updated" => (
            format!(
                "Forum category updated: {}",
                target.as_deref().unwrap_or("a category")
            ),
            "folder".to_string(),
        ),
        "forum_category_deleted" => (
            format!(
                "Forum category deleted: {}",
                target.as_deref().unwrap_or("a category")
            ),
            "folder-minus".to_string(),
        ),
        "personal_forum_created" => (
            format!(
                "Personal forum created by {}",
                user_id.as_deref().unwrap_or("someone")
            ),
            "user".to_string(),
        ),
        "project_saved" => (
            format!("Saved project: {}", target.as_deref().unwrap_or("an item")),
            "bookmark".to_string(),
        ),
        "project_joined" => (
            format!(
                "Joined project: {}",
                target.as_deref().unwrap_or("a project")
            ),
            "users".to_string(),
        ),
        "pageview" => (
            format!(
                "{} viewed {}",
                user_id.as_deref().unwrap_or("someone"),
                target.as_deref().unwrap_or("a page"),
            ),
            "eye".to_string(),
        ),
        "clickthrough" => (
            format!(
                "{} clicked through to {}",
                user_id.as_deref().unwrap_or("someone"),
                target.as_deref().unwrap_or("external site")
            ),
            "external-link".to_string(),
        ),
        _ => {
            // Format unknown activities nicely
            let description = match (user_id, target) {
                (Some(user), Some(tgt)) => format!("User {} performed {}: {}", user, action, tgt),
                (Some(user), None) => format!("User {} performed {}", user, action),
                (None, Some(tgt)) => format!("{} on {}", action.replace('.', " "), tgt),
                (None, None) => format!("{}", action.replace('.', " ")),
            };
            (description, "activity".to_string())
        }
    }
}

/// Format timestamp to human-readable time ago
fn format_time_ago(when: &str) -> String {
    if let Ok(parsed_time) = jiff::Timestamp::from_str(when) {
        let now = jiff::Timestamp::now();
        if let Ok(duration) = now.since(parsed_time) {
            let total_seconds = duration.total(jiff::Unit::Second).unwrap_or(0.0);

            if total_seconds < 60.0 {
                format!("{} seconds ago", total_seconds as i64)
            } else if total_seconds < 3600.0 {
                format!("{} minutes ago", (total_seconds / 60.0) as i64)
            } else if total_seconds < 86400.0 {
                format!("{} hours ago", (total_seconds / 3600.0) as i64)
            } else {
                format!("{} days ago", (total_seconds / 86400.0) as i64)
            }
        } else {
            "unknown time".to_string()
        }
    } else {
        "unknown time".to_string()
    }
}
