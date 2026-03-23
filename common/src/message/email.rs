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
    pub subject: String, // Email subject (will be processed for merge fields)
    pub recipients: Vec<EmailRecipient>,
    pub data_sets: Vec<String>, // Names of data sets to use
    pub additional_data: Option<Vec<(String, String)>>, // Custom merge fields
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmailRecipient {
    Email(String),
    UserId(Ulid),
    UserSet { slug: String },
    AddressSet { slug: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendEmailResponse {
    pub sending_id: Ulid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddTargetsToSendingRequest {
    pub sending_id: Ulid,
    pub recipients: Vec<EmailRecipient>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddTargetsToSendingResponse {
    pub success: bool,
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
    pub recipients: Option<String>,
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
    pub min_queued: Option<u32>,
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
    pub ses_message_id: Option<String>,
    pub user_id: Option<Ulid>,
    pub email_address: String,
    pub bounce_type: String, // 'permanent', 'temporary', 'undetermined'
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplaintEvent {
    pub sending_id: Option<Ulid>,
    pub ses_message_id: Option<String>,
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

// ============================================================================
// Stored Email Messages
// ============================================================================

#[cfg(feature = "database")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEmail {
    pub stored_email_id: Ulid,
    pub slug: String,
    pub subject: String,
    pub body_delta: String,    // Quill Delta JSON
    pub datasets: Vec<String>, // Dataset names
    pub created_at: TimeString,
    pub updated_at: TimeString,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListStoredEmailsRequest {
    pub page: u32,
    pub per_page: u32,
    pub search: Option<String>,
}

#[cfg(feature = "database")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListStoredEmailsResponse {
    pub emails: Vec<StoredEmail>,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetStoredEmailRequest {
    pub stored_email_id: Ulid,
}

#[cfg(feature = "database")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetStoredEmailResponse {
    pub email: StoredEmail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateStoredEmailRequest {
    pub slug: String,
    pub subject: String,
    pub body_delta: String,    // Quill Delta JSON
    pub datasets: Vec<String>, // Dataset names
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateStoredEmailResponse {
    pub stored_email_id: Ulid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStoredEmailRequest {
    pub stored_email_id: Ulid,
    pub slug: Option<String>,
    pub subject: Option<String>,
    pub body_delta: Option<String>,    // Quill Delta JSON
    pub datasets: Option<Vec<String>>, // Dataset names
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStoredEmailResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteStoredEmailRequest {
    pub stored_email_id: Ulid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteStoredEmailResponse {
    pub success: bool,
}

// ============================================================================
// Oneoff Email Messages
// ============================================================================

#[cfg(feature = "database")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneoffEmail {
    pub oneoff_email_id: Ulid,
    pub subject: String,
    pub body_delta: String,    // Quill Delta JSON
    pub datasets: Vec<String>, // Dataset names
    pub created_at: TimeString,
    pub archived: bool,
    pub legacy_slug: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListOneoffEmailsRequest {
    pub page: u32,
    pub per_page: u32,
    pub include_archived: bool,
}

#[cfg(feature = "database")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListOneoffEmailsResponse {
    pub emails: Vec<OneoffEmail>,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetOneoffEmailRequest {
    pub oneoff_email_id: Ulid,
}

#[cfg(feature = "database")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetOneoffEmailResponse {
    pub email: OneoffEmail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOneoffEmailRequest {
    pub subject: String,
    pub body_delta: String,    // Quill Delta JSON
    pub datasets: Vec<String>, // Dataset names
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOneoffEmailResponse {
    pub oneoff_email_id: Ulid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateOneoffEmailRequest {
    pub oneoff_email_id: Ulid,
    pub subject: Option<String>,
    pub body_delta: Option<String>,    // Quill Delta JSON
    pub datasets: Option<Vec<String>>, // Dataset names
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateOneoffEmailResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteOneoffEmailRequest {
    pub oneoff_email_id: Ulid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteOneoffEmailResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveOneoffEmailRequest {
    pub oneoff_email_id: Ulid,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveOneoffEmailResponse {
    pub success: bool,
}

// ============================================================================
// Email Trigger Messages
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerActionType {
    Immediate,
    Delayed,
}

impl TriggerActionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Immediate => "immediate",
            Self::Delayed => "delayed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "immediate" => Some(Self::Immediate),
            "delayed" => Some(Self::Delayed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerActionTaken {
    SentImmediately,
    Scheduled,
    CancelledPending,
    NoTrigger,
}

/// Fire a trigger event to potentially send an email
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerEmailRequest {
    pub trigger_name: String,
    pub user_id: Ulid,
    pub additional_data: Option<Vec<(String, String)>>, // Extra merge fields
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerEmailResponse {
    pub success: bool,
    pub action_taken: TriggerActionTaken,
}

/// Trigger configuration
#[cfg(feature = "database")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerInfo {
    pub trigger_id: Ulid,
    pub trigger_name: String,
    pub stored_email_id: Ulid,
    pub action_type: TriggerActionType,
    pub delay_seconds: Option<i64>,
    pub cancellation_trigger_name: Option<String>,
    pub recipients: Option<String>,
    pub active: bool,
    pub created_at: TimeString,
    pub updated_at: TimeString,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTriggerRequest {
    pub trigger_name: String,
    pub stored_email_id: Ulid,
    pub action_type: TriggerActionType,
    pub delay_seconds: Option<i64>,
    pub cancellation_trigger_name: Option<String>,
    pub recipients: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTriggerResponse {
    pub trigger_id: Ulid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTriggerRequest {
    pub trigger_id: Ulid,
    pub stored_email_id: Option<Ulid>,
    pub action_type: Option<TriggerActionType>,
    pub delay_seconds: Option<i64>,
    pub cancellation_trigger_name: Option<String>,
    pub recipients: Option<String>,
    pub active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTriggerResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteTriggerRequest {
    pub trigger_id: Ulid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteTriggerResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTriggersRequest {
    pub active_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTriggersResponse {
    #[cfg(feature = "database")]
    pub triggers: Vec<TriggerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetTriggerRequest {
    pub trigger_id: Ulid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetTriggerResponse {
    #[cfg(feature = "database")]
    pub trigger: TriggerInfo,
}
