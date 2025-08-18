use anyhow::Result;
use inventory;
use log_service::database::storage::Database;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Progress callback function type
pub type ProgressCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Trait for analytics functions
pub trait AnalyticsFunction: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn usage_example(&self) -> &'static str;
    fn execute(
        &self,
        database: Arc<Database>,
        parameters: HashMap<String, String>,
        progress: ProgressCallback,
    ) -> Pin<Box<dyn Future<Output = Result<(Vec<String>, Vec<Vec<String>>)>> + Send + '_>>;
    fn validate(&self, _parameters: &HashMap<String, String>) -> Result<()> {
        // Default: no validation
        Ok(())
    }
}

/// Registration wrapper for analytics functions
pub struct AnalyticsFunctionRegistration {
    pub function: &'static dyn AnalyticsFunction,
}

inventory::collect!(AnalyticsFunctionRegistration);

/// Get all registered analytics functions
pub fn get_registered_functions() -> Vec<&'static dyn AnalyticsFunction> {
    inventory::iter::<AnalyticsFunctionRegistration>()
        .map(|registration| registration.function)
        .collect()
}

/// Find a registered function by name
pub fn find_function(name: &str) -> Option<&'static dyn AnalyticsFunction> {
    inventory::iter::<AnalyticsFunctionRegistration>()
        .map(|registration| registration.function)
        .find(|f| f.name() == name)
}

/// Validate parameters using the function's validator
pub fn validate_parameters(
    func: &dyn AnalyticsFunction,
    parameters: &HashMap<String, String>,
) -> Result<()> {
    func.validate(parameters)
}

/// Common validation helper for date format (YYYY-MM-DD)
pub fn validate_date_format(date_str: &str) -> Result<()> {
    if date_str.len() != 10 {
        return Err(anyhow::anyhow!("Date must be 10 characters long"));
    }
    let parts: Vec<&str> = date_str.split('-').collect();
    if parts.len() != 3 {
        return Err(anyhow::anyhow!("Date must be in YYYY-MM-DD format"));
    }
    // Check if year, month, day are all numeric
    if !(parts[0].len() == 4
        && parts[0].chars().all(|c| c.is_ascii_digit())
        && parts[1].len() == 2
        && parts[1].chars().all(|c| c.is_ascii_digit())
        && parts[2].len() == 2
        && parts[2].chars().all(|c| c.is_ascii_digit()))
    {
        return Err(anyhow::anyhow!(
            "Date must be in YYYY-MM-DD format with valid numbers"
        ));
    }
    Ok(())
}

/// Helper to validate analytics query parameters (start_date, end_date)
pub fn validate_analytics_params(parameters: &HashMap<String, String>) -> Result<()> {
    let known_params = ["start_date", "end_date"];

    // Check for unknown parameters
    for param_name in parameters.keys() {
        if !known_params.contains(&param_name.as_str()) {
            return Err(anyhow::anyhow!(
                "Unknown parameter '{}'. Valid parameters: {}",
                param_name,
                known_params.join(", ")
            ));
        }
    }

    // Validate date formats
    if let Some(start_date) = parameters.get("start_date") {
        validate_date_format(start_date)
            .map_err(|e| anyhow::anyhow!("Invalid start_date: {}", e))?;
    }

    if let Some(end_date) = parameters.get("end_date") {
        validate_date_format(end_date).map_err(|e| anyhow::anyhow!("Invalid end_date: {}", e))?;
    }

    Ok(())
}
