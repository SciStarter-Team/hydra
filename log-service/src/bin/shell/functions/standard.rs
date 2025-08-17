use super::convert_analytics_response_to_table;
use crate::query_engine::functions::validate_analytics_params;
use crate::query_engine::functions::{AnalyticsFunction, AnalyticsFunctionRegistration};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use log_service::database::storage::Database;
use std::pin::Pin;
use std::future::Future;

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
    
    fn execute(&self, database: Arc<Database>, parameters: HashMap<String, String>) -> Pin<Box<dyn Future<Output = Result<(Vec<String>, Vec<Vec<String>>)>> + Send + '_>> {
        Box::pin(async move {
            use log_service::analytics::get_clickthrough_analytics;
            use common::message::log::AnalyticsQuery;
            
            let start_date = parameters.get("start_date")
                .cloned()
                .unwrap_or_else(|| "2024-01-01".to_string());
            let end_date = parameters.get("end_date")
                .cloned()
                .unwrap_or_else(|| "2024-12-31".to_string());
            
            let query = AnalyticsQuery { start_date, end_date };
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
    
    fn execute(&self, database: Arc<Database>, parameters: HashMap<String, String>) -> Pin<Box<dyn Future<Output = Result<(Vec<String>, Vec<Vec<String>>)>> + Send + '_>> {
        Box::pin(async move {
            use log_service::analytics::get_join_analytics;
            use common::message::log::AnalyticsQuery;
            
            let start_date = parameters.get("start_date")
                .cloned()
                .unwrap_or_else(|| "2024-01-01".to_string());
            let end_date = parameters.get("end_date")
                .cloned()
                .unwrap_or_else(|| "2024-12-31".to_string());
            
            let query = AnalyticsQuery { start_date, end_date };
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

inventory::submit! {
    AnalyticsFunctionRegistration {
        function: &JoinAnalytics,
    }
}