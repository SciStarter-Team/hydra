use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[cfg(feature = "database")]
use crate::timestring::TimeString;

// ============================================================================
// Send Request Messages
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendEmailRequest {
    pub template_html: String,
    pub subject: String,         // Email subject (will be processed for merge fields)
    pub recipients: Vec<EmailRecipient>,
    pub data_sets: Vec<String>, // Names of data sets to use
    pub additional_data: Option<Vec<(String, String)>>, // Custom merge fields
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmailRecipient {
    Email(String),
    UserId(Ulid),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendEmailResponse {
    pub sending_id: Ulid,
    pub total_queued: u32,
}

// ============================================================================
// Scheduled Send Messages
// ============================================================================

#[cfg(feature = "database")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleSendRequest {
    pub user_id: Ulid,
    pub hook: String,
    pub scheduled_time: TimeString,
    pub template_html: String,
    pub subject: String,
    pub data_sets: Vec<String>,
    pub additional_data: Option<Vec<(String, String)>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleSendResponse {
    pub scheduled_id: Ulid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelScheduledSendRequest {
    pub user_id: Ulid,
    pub hook: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelScheduledSendResponse {
    pub cancelled_count: u32,
}

// ============================================================================
// Analytics Messages
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStatusRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStatusResponse {
    pub pending_count: u32,
    pub processing_count: u32,
    pub estimated_completion_seconds: u32, // Based on current rate
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSendingsRequest {
    pub page: u32,
    pub per_page: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSendingsResponse {
    pub sendings: Vec<SendingMetrics>,
    pub total: u32,
}

#[cfg(feature = "database")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendingMetrics {
    pub sending_id: Ulid,
    pub created_at: TimeString,
    pub subject: String,
    pub total_queued: u32,
    pub messages_sent: u32,
    pub messages_skipped: u32,
    pub messages_opened: u32,
    pub messages_bounced: u32,
    pub unsubscribes: u32,
    pub complaints: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSendingRequest {
    pub sending_id: Ulid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSendingResponse {
    #[cfg(feature = "database")]
    pub metrics: SendingMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckSendabilityRequest {
    pub email_address: String,
}

#[cfg(feature = "database")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckSendabilityResponse {
    pub can_send: bool,
    pub reason: Option<String>, // If can_send=false: 'bounce', 'complaint', 'unsubscribe'
    pub suppressed_at: Option<TimeString>,
}

// ============================================================================
// Event Messages
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BounceEvent {
    pub sending_id: Option<Ulid>,
    pub user_id: Option<Ulid>,
    pub email_address: String,
    pub bounce_type: String, // 'permanent', 'temporary', 'undetermined'
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplaintEvent {
    pub sending_id: Option<Ulid>,
    pub user_id: Option<Ulid>,
    pub email_address: String,
    pub complaint_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeEvent {
    pub sending_id: Option<Ulid>,
    pub user_id: Option<Ulid>,
    pub email_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenEvent {
    pub sending_id: Ulid,
    pub user_id: Option<Ulid>,
    pub email_address: String,
}

// ============================================================================
// Dataset Introspection Messages
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDatasetsRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetInfo {
    pub name: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDatasetsResponse {
    pub datasets: Vec<DatasetInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetDatasetVariablesRequest {
    pub dataset_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableInfo {
    pub name: String,
    pub description: String,
    pub example: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetDatasetVariablesResponse {
    pub variables: Vec<VariableInfo>,
}
