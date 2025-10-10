use anyhow::Error;
use async_trait::async_trait;
use common::{message::email::VariableInfo, service::request};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use ulid::Ulid;

/// Trait for data sets that provide merge field values for email templates
#[async_trait]
pub trait DataSet {
    /// The internal identifier for this dataset (e.g., "user_identity")
    fn name(&self) -> &'static str;

    /// The user-friendly label for this dataset (e.g., "User Identity")
    fn label(&self) -> &'static str;

    /// A brief description of this dataset
    fn description(&self) -> &'static str;

    /// Returns information about the variables provided by this dataset
    fn variables(&self) -> Vec<VariableInfo>;

    /// Returns (field_names, field_values) for a given user
    async fn get_values(&self, user_id: Ulid) -> Result<(Vec<String>, Vec<String>), Error>;
}

/// Global registry of available data sets
pub static DATASET_REGISTRY: LazyLock<HashMap<String, Arc<dyn DataSet + Send + Sync>>> =
    LazyLock::new(|| {
        let mut datasets: HashMap<String, Arc<dyn DataSet + Send + Sync>> = HashMap::new();

        // Register all available data sets
        datasets.insert("user_identity".to_string(), Arc::new(UserIdentity));

        datasets
    });

/// Data set that retrieves user identity information
pub struct UserIdentity;

#[async_trait]
impl DataSet for UserIdentity {
    fn name(&self) -> &'static str {
        "user_identity"
    }

    fn label(&self) -> &'static str {
        "User Identity"
    }

    fn description(&self) -> &'static str {
        "Basic user identity information including name, email, and handle"
    }

    fn variables(&self) -> Vec<VariableInfo> {
        vec![
            VariableInfo {
                name: "FIRST_NAME".to_string(),
                description: "User's first name".to_string(),
                example: "Jane".to_string(),
            },
            VariableInfo {
                name: "LAST_NAME".to_string(),
                description: "User's last name".to_string(),
                example: "Smith".to_string(),
            },
            VariableInfo {
                name: "EMAIL".to_string(),
                description: "User's primary email address".to_string(),
                example: "jane.smith@example.com".to_string(),
            },
            VariableInfo {
                name: "HANDLE".to_string(),
                description: "User's unique handle/username".to_string(),
                example: "janesmith42".to_string(),
            },
            VariableInfo {
                name: "FRIENDLY_NAME".to_string(),
                description: "User's first name if available, otherwise their handle".to_string(),
                example: "Jane".to_string(),
            },
        ]
    }

    async fn get_values(&self, user_id: Ulid) -> Result<(Vec<String>, Vec<String>), Error> {
        use common::message::user::{AdminUser, AdminUserGetRequest};

        // NATS call to user-service to get detailed user info
        let user = request::<AdminUser>("user.admin.get", AdminUserGetRequest { user_id }).await?;

        // Derive friendly name: use first_name if not empty, otherwise handle
        let friendly_name = if !user.first_name.is_empty() {
            user.first_name.clone()
        } else {
            user.handle.clone()
        };

        Ok((
            vec![
                "FIRST_NAME".to_string(),
                "LAST_NAME".to_string(),
                "EMAIL".to_string(),
                "HANDLE".to_string(),
                "FRIENDLY_NAME".to_string(),
            ],
            vec![
                user.first_name,
                user.last_name,
                user.primary_email,
                user.handle,
                friendly_name,
            ],
        ))
    }
}
