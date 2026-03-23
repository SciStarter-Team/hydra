use super::convert_analytics_response_to_table;
use crate::query_engine::functions::validate_analytics_params;
use crate::query_engine::functions::{AnalyticsFunction, AnalyticsFunctionRegistration};
use anyhow::Result;
use log_service::database::storage::Database;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Clickthrough analytics function
pub struct ClickthroughAnalytics;

impl AnalyticsFunction for ClickthroughAnalytics {
    fn name(&self) -> &'static str {
        "clickthrough_analytics"
    }

    fn description(&self) -> &'static str {
        "Get clickthrough analytics with weekly aggregation"
    }

    fn usage_example(&self) -> &'static str {
        "clickthrough_analytics(start_date=\"2024-01-01\", end_date=\"2024-12-31\")"
    }

    fn execute(
        &self,
        database: Arc<Database>,
        parameters: HashMap<String, String>,
        _progress: crate::query_engine::functions::ProgressCallback,
    ) -> Pin<Box<dyn Future<Output = Result<(Vec<String>, Vec<Vec<String>>)>> + Send + '_>> {
        Box::pin(async move {
            use common::message::log::AnalyticsQuery;
            use log_service::analytics::get_clickthrough_analytics;

            let start_date = parameters
                .get("start_date")
                .cloned()
                .unwrap_or_else(|| "2024-01-01".to_string());
            let end_date = parameters
                .get("end_date")
                .cloned()
                .unwrap_or_else(|| "2024-12-31".to_string());

            let query = AnalyticsQuery {
                start_date,
                end_date,
            };
            let response = get_clickthrough_analytics(database, query).await?;
            convert_analytics_response_to_table(response, "Clickthrough Analytics")
        })
    }

    fn validate(&self, parameters: &HashMap<String, String>) -> Result<()> {
        validate_analytics_params(parameters)
    }
}

/// Join analytics function
pub struct JoinAnalytics;

impl AnalyticsFunction for JoinAnalytics {
    fn name(&self) -> &'static str {
        "join_analytics"
    }

    fn description(&self) -> &'static str {
        "Get join analytics with weekly aggregation"
    }

    fn usage_example(&self) -> &'static str {
        "join_analytics(start_date=\"2024-01-01\", end_date=\"2024-12-31\")"
    }

    fn execute(
        &self,
        database: Arc<Database>,
        parameters: HashMap<String, String>,
        _progress: crate::query_engine::functions::ProgressCallback,
    ) -> Pin<Box<dyn Future<Output = Result<(Vec<String>, Vec<Vec<String>>)>> + Send + '_>> {
        Box::pin(async move {
            use common::message::log::AnalyticsQuery;
            use log_service::analytics::get_join_analytics;

            let start_date = parameters
                .get("start_date")
                .cloned()
                .unwrap_or_else(|| "2024-01-01".to_string());
            let end_date = parameters
                .get("end_date")
                .cloned()
                .unwrap_or_else(|| "2024-12-31".to_string());

            let query = AnalyticsQuery {
                start_date,
                end_date,
            };
            let response = get_join_analytics(database, query).await?;
            convert_analytics_response_to_table(response, "Join Analytics")
        })
    }

    fn validate(&self, parameters: &HashMap<String, String>) -> Result<()> {
        validate_analytics_params(parameters)
    }
}

// Register the functions using inventory
inventory::submit! {
    AnalyticsFunctionRegistration {
        function: &ClickthroughAnalytics,
    }
}

/// Initialize cache function - clears and re-initializes cache tables
pub struct InitializeCache;

impl AnalyticsFunction for InitializeCache {
    fn name(&self) -> &'static str {
        "initialize_cache"
    }

    fn description(&self) -> &'static str {
        "Clear and re-initialize cache tables with fresh data"
    }

    fn usage_example(&self) -> &'static str {
        "initialize_cache"
    }

    fn execute(
        &self,
        _database: Arc<Database>,
        _parameters: HashMap<String, String>,
        progress: crate::query_engine::functions::ProgressCallback,
    ) -> Pin<Box<dyn Future<Output = Result<(Vec<String>, Vec<Vec<String>>)>> + Send + '_>> {
        Box::pin(async move {
            use common::service::request;

            let db = common::shared::db();

            progress("Starting cache synchronization...".to_string());

            // Clear both cache tables
            progress("Clearing existing cache data...".to_string());

            let tags_cleared = sqlx::query!(r#"DELETE FROM "user_tags_cache""#)
                .execute(db)
                .await?
                .rows_affected();

            let emails_cleared = sqlx::query!(r#"DELETE FROM "user_emails_cache""#)
                .execute(db)
                .await?
                .rows_affected();

            progress(format!(
                "Cleared {} tags and {} emails from cache",
                tags_cleared, emails_cleared
            ));

            let mut total_tags = 0u64;
            let mut total_emails = 0u64;
            let now = jiff::Timestamp::now().to_string();

            // Sync tags with pagination
            progress("Fetching user tags from user service...".to_string());

            let mut offset = 0i64;
            const BATCH_SIZE: i64 = 1000;

            loop {
                let sync_req = serde_json::json!({
                    "offset": offset,
                    "limit": BATCH_SIZE
                });

                let tags: Vec<(String, String)> = request("user.cache.user_tags", sync_req).await?;

                if tags.is_empty() {
                    break;
                }

                // Insert tags into cache
                for (user_id, tag) in &tags {
                    sqlx::query!(
                        r#"INSERT INTO "user_tags_cache" ("user_id", "tag", "updated_at") VALUES (?, ?, ?)"#,
                        user_id,
                        tag,
                        now
                    )
                    .execute(db)
                    .await?;
                }

                total_tags += tags.len() as u64;
                offset += BATCH_SIZE;

                progress(format!("Loaded {} total tags so far", total_tags));
            }

            // Sync emails with pagination
            progress("Fetching user emails from user service...".to_string());

            offset = 0;

            loop {
                let sync_req = serde_json::json!({
                    "offset": offset,
                    "limit": BATCH_SIZE
                });

                let emails: Vec<(String, String)> =
                    request("user.cache.user_emails", sync_req).await?;

                if emails.is_empty() {
                    break;
                }

                // Insert emails into cache
                for (user_id, email) in &emails {
                    sqlx::query!(
                        r#"INSERT INTO "user_emails_cache" ("user_id", "email", "updated_at") VALUES (?, ?, ?)"#,
                        user_id,
                        email,
                        now
                    )
                    .execute(db)
                    .await?;
                }

                total_emails += emails.len() as u64;
                offset += BATCH_SIZE;

                progress(format!("Loaded {} total emails so far", total_emails));
            }

            progress("Cache synchronization completed!".to_string());

            let headers = vec![
                "Table".to_string(),
                "Cleared".to_string(),
                "Loaded".to_string(),
                "Status".to_string(),
            ];
            let mut rows = Vec::new();

            rows.push(vec![
                "user_tags_cache".to_string(),
                tags_cleared.to_string(),
                total_tags.to_string(),
                "Synced".to_string(),
            ]);

            rows.push(vec![
                "user_emails_cache".to_string(),
                emails_cleared.to_string(),
                total_emails.to_string(),
                "Synced".to_string(),
            ]);

            Ok((headers, rows))
        })
    }

    fn validate(&self, parameters: &HashMap<String, String>) -> Result<()> {
        // This function takes no parameters
        if !parameters.is_empty() {
            return Err(anyhow::anyhow!("initialize_cache takes no parameters"));
        }
        Ok(())
    }
}

inventory::submit! {
    AnalyticsFunctionRegistration {
        function: &JoinAnalytics,
    }
}

inventory::submit! {
    AnalyticsFunctionRegistration {
        function: &InitializeCache,
    }
}

/// Regenerate summaries function - rebuilds all summary tables from scratch
pub struct RegenerateSummaries;

impl AnalyticsFunction for RegenerateSummaries {
    fn name(&self) -> &'static str {
        "regenerate_summaries"
    }

    fn description(&self) -> &'static str {
        "Regenerate all summary tables from scratch using all available parquet data"
    }

    fn usage_example(&self) -> &'static str {
        "regenerate_summaries"
    }

    fn execute(
        &self,
        database: Arc<Database>,
        _parameters: HashMap<String, String>,
        progress: crate::query_engine::functions::ProgressCallback,
    ) -> Pin<Box<dyn Future<Output = Result<(Vec<String>, Vec<Vec<String>>)>> + Send + '_>> {
        Box::pin(async move {
            use log_service::database::summaries::regenerate_all_summaries;

            progress("Starting full regeneration of summary tables...".to_string());

            let result = regenerate_all_summaries(database).await;

            match result {
                Ok(_) => {
                    progress("Summary regeneration completed successfully!".to_string());

                    let headers = vec![
                        "Operation".to_string(),
                        "Status".to_string(),
                        "Details".to_string(),
                    ];
                    let rows = vec![vec![
                        "Regenerate Summaries".to_string(),
                        "Success".to_string(),
                        "All summary tables rebuilt from scratch".to_string(),
                    ]];

                    Ok((headers, rows))
                }
                Err(e) => {
                    let error_msg = format!("Summary regeneration failed: {}", e);
                    progress(error_msg.clone());

                    let headers = vec![
                        "Operation".to_string(),
                        "Status".to_string(),
                        "Error".to_string(),
                    ];
                    let rows = vec![vec![
                        "Regenerate Summaries".to_string(),
                        "Failed".to_string(),
                        e.to_string(),
                    ]];

                    Ok((headers, rows))
                }
            }
        })
    }

    fn validate(&self, parameters: &HashMap<String, String>) -> Result<()> {
        // This function takes no parameters
        if !parameters.is_empty() {
            return Err(anyhow::anyhow!("regenerate_summaries takes no parameters"));
        }
        Ok(())
    }
}

inventory::submit! {
    AnalyticsFunctionRegistration {
        function: &RegenerateSummaries,
    }
}

/// Initialize completion cache function - populates cache tables from existing data
pub struct InitCompletionCache;

impl AnalyticsFunction for InitCompletionCache {
    fn name(&self) -> &'static str {
        "init_completion_cache"
    }

    fn description(&self) -> &'static str {
        "Initialize articulate_completions_cache and project_participations_cache from existing data"
    }

    fn usage_example(&self) -> &'static str {
        "init_completion_cache()"
    }

    fn execute(
        &self,
        database: Arc<Database>,
        _parameters: HashMap<String, String>,
        progress: crate::query_engine::functions::ProgressCallback,
    ) -> Pin<Box<dyn Future<Output = Result<(Vec<String>, Vec<Vec<String>>)>> + Send + '_>> {
        Box::pin(async move {
            use datafusion::arrow::array::{Array, StringArray};
            use datafusion::prelude::*;

            let ctx = database.context();
            let db = common::shared::db();

            progress("Initializing completion cache tables from existing data...".to_string());

            // 1. Initialize articulate_completions_cache from activity_log
            progress("Processing articulate completions from activity_log...".to_string());

            let df = ctx
                .table("activity_log")
                .await?
                .filter(col("action").eq(lit("articulate_complete")))?
                .filter(col("user_id").is_not_null())?
                .filter(col("extra").is_not_null())?
                .select(vec![col("user_id"), col("extra").alias("activity")])?
                .distinct()?;

            let batches = df.collect().await?;
            let mut articulate_count = 0;

            for batch in batches {
                if batch.num_rows() == 0 {
                    continue;
                }

                let user_id_array = batch
                    .column(0)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .expect("user_id column should be StringArray");

                let activity_array = batch
                    .column(1)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .expect("activity column should be StringArray");

                for i in 0..batch.num_rows() {
                    if !user_id_array.is_null(i) && !activity_array.is_null(i) {
                        let user_id = user_id_array.value(i);
                        let activity = activity_array.value(i);

                        sqlx::query!(
                            r#"
                            INSERT OR IGNORE INTO articulate_completions_cache (user_id, activity)
                            VALUES (?, ?)
                            "#,
                            user_id,
                            activity
                        )
                        .execute(db)
                        .await?;

                        articulate_count += 1;
                    }
                }

                if articulate_count % 100 == 0 {
                    progress(format!(
                        "Processed {} articulate completions...",
                        articulate_count
                    ));
                }
            }

            progress(format!(
                "Completed articulate completions: {} records",
                articulate_count
            ));

            // 2. Initialize project_participations_cache from participation
            progress("Processing project participations from participation table...".to_string());

            let df = ctx
                .table("participation")
                .await?
                .filter(col("user_id").is_not_null())?
                .select(vec![col("user_id"), col("entity_id").alias("project_id")])?
                .distinct()?;

            let batches = df.collect().await?;
            let mut participation_count = 0;

            for batch in batches {
                if batch.num_rows() == 0 {
                    continue;
                }

                let user_id_array = batch
                    .column(0)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .expect("user_id column should be StringArray");

                let project_id_array = batch
                    .column(1)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .expect("project_id column should be StringArray");

                for i in 0..batch.num_rows() {
                    if !user_id_array.is_null(i) && !project_id_array.is_null(i) {
                        let user_id = user_id_array.value(i);
                        let project_id = project_id_array.value(i);

                        sqlx::query!(
                            r#"
                            INSERT OR IGNORE INTO project_participations_cache (user_id, project_id)
                            VALUES (?, ?)
                            "#,
                            user_id,
                            project_id
                        )
                        .execute(db)
                        .await?;

                        participation_count += 1;
                    }
                }

                if participation_count % 100 == 0 {
                    progress(format!(
                        "Processed {} project participations...",
                        participation_count
                    ));
                }
            }

            progress(format!(
                "Completed project participations: {} records",
                participation_count
            ));

            progress("Completion cache initialization completed!".to_string());

            let headers = vec![
                "Table".to_string(),
                "Records Loaded".to_string(),
                "Status".to_string(),
            ];

            let rows = vec![
                vec![
                    "articulate_completions_cache".to_string(),
                    articulate_count.to_string(),
                    "Initialized".to_string(),
                ],
                vec![
                    "project_participations_cache".to_string(),
                    participation_count.to_string(),
                    "Initialized".to_string(),
                ],
            ];

            Ok((headers, rows))
        })
    }

    fn validate(&self, parameters: &HashMap<String, String>) -> Result<()> {
        // This function takes no parameters
        if !parameters.is_empty() {
            return Err(anyhow::anyhow!("init_completion_cache takes no parameters"));
        }
        Ok(())
    }
}

inventory::submit! {
    AnalyticsFunctionRegistration {
        function: &InitCompletionCache,
    }
}
