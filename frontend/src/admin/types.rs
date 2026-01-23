// Shared types and helpers for admin modules

use common::message::user::{UserDetailsGetRequest, UserDetailsResponse};
use common::service::request;
use ulid::Ulid;

/// Helper function to look up user email by ULID
pub async fn lookup_user_email(user_id: Option<Ulid>) -> String {
    match user_id {
        Some(id) => {
            let get_request = UserDetailsGetRequest { user_id: id };
            match request::<UserDetailsResponse>("user.admin.user_get", get_request).await {
                Ok(response) => response.user.primary_email,
                Err(_) => id.to_string(), // Fallback to ULID if lookup fails
            }
        }
        None => String::new(),
    }
}
