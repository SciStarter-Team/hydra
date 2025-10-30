use std::collections::BTreeMap;
use std::sync::Arc;

use crate::badge::Badge;
use crate::timestring::TimeString;
use anyhow::Error;
use serde_json::Value as JsonValue;
use ulid::Ulid;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Signup {
    pub handle: String,
    pub email: String,
    pub password: String,
    pub newsletter: bool,
    pub zipcode: String,
    pub protected: bool,
    pub point: Option<String>,
    pub tags: Vec<String>,
    pub disabled_password: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Login {
    pub email: String,
    pub password: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Session {
    pub session_id: String,
    pub expires: Option<String>,
    pub data: Box<[u8]>,
}

impl Session {
    pub fn new(
        session_id: impl Into<String>,
        expires: impl Into<Option<String>>,
        data: &BTreeMap<String, JsonValue>,
    ) -> Result<Session, Error> {
        Ok(Session {
            session_id: session_id.into(),
            expires: expires.into(),
            data: flexbuffers::to_vec(data)?.into_boxed_slice(),
        })
    }

    pub fn decode(&self) -> Result<BTreeMap<String, JsonValue>, Error> {
        Ok(flexbuffers::from_slice(&self.data)?)
    }

    pub fn encode(&mut self, data: BTreeMap<String, JsonValue>) -> Result<(), Error> {
        self.data = flexbuffers::to_vec(data)?.into_boxed_slice();
        Ok(())
    }
}

pub trait UserInfo {
    fn user_id(&self) -> Option<Ulid>;
    fn handle(&self) -> &str;
    fn is_staff(&self) -> bool;
    fn is_superuser(&self) -> bool;

    fn can_access_admin(&self) -> bool {
        self.is_staff() | self.is_superuser()
    }
}

pub struct Unauthenticated;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BasicInfo {
    pub user_id: Ulid,
    pub handle: String,
    pub is_staff: bool,
    pub is_superuser: bool,
    pub public: bool,
    pub protected: bool,
    pub needs_point: bool,
}

impl UserInfo for Unauthenticated {
    fn user_id(&self) -> Option<Ulid> {
        None
    }

    fn handle(&self) -> &str {
        "Unknown"
    }

    fn is_staff(&self) -> bool {
        false
    }

    fn is_superuser(&self) -> bool {
        false
    }
}

impl BasicInfo {
    pub fn is_private(&self) -> bool {
        !self.public || self.protected
    }

    pub fn display_handle(&self) -> &str {
        if self.is_private() {
            "Private"
        } else if self.handle.trim().is_empty() {
            "Nemo" // Maybe this should just be "No Handle", "Nemo" is kind of quirky
        } else {
            &self.handle
        }
    }
}

impl UserInfo for BasicInfo {
    fn user_id(&self) -> Option<Ulid> {
        Some(self.user_id)
    }

    fn handle(&self) -> &str {
        self.handle.as_str()
    }

    fn is_staff(&self) -> bool {
        self.is_staff || self.is_superuser
    }

    fn is_superuser(&self) -> bool {
        self.is_superuser
    }
}

impl UserInfo for Option<BasicInfo> {
    fn user_id(&self) -> Option<Ulid> {
        match self {
            Some(info) => info.user_id(),
            None => Unauthenticated.user_id(),
        }
    }

    fn handle(&self) -> &str {
        match self {
            Some(info) => info.handle(),
            None => Unauthenticated.handle(),
        }
    }

    fn is_staff(&self) -> bool {
        match self {
            Some(info) => info.is_staff(),
            None => Unauthenticated.is_staff(),
        }
    }

    fn is_superuser(&self) -> bool {
        match self {
            Some(info) => info.is_superuser(),
            None => Unauthenticated.is_superuser(),
        }
    }
}

impl UserInfo for Option<&BasicInfo> {
    fn user_id(&self) -> Option<Ulid> {
        match self {
            Some(info) => info.user_id(),
            None => Unauthenticated.user_id(),
        }
    }

    fn handle(&self) -> &str {
        match self {
            Some(info) => info.handle(),
            None => Unauthenticated.handle(),
        }
    }

    fn is_staff(&self) -> bool {
        match self {
            Some(info) => info.is_staff(),
            None => Unauthenticated.is_staff(),
        }
    }

    fn is_superuser(&self) -> bool {
        match self {
            Some(info) => info.is_superuser(),
            None => Unauthenticated.is_superuser(),
        }
    }
}

pub trait UserDisplayStr {
    fn display_str(&self) -> String;
}

impl UserDisplayStr for Option<Arc<BasicInfo>> {
    fn display_str(&self) -> String {
        match self {
            Some(user) => user.user_id.to_string(),
            None => "anonymous".to_string(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserBadge {
    pub badge: Badge,
    pub when: TimeString, // Timestamp when badge was earned
    pub image: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserAndBadge {
    pub user_id: Ulid,
    pub badge: Badge,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserCountResponse {
    pub count: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminUserListRequest {
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminUser {
    pub user_id: Ulid,
    pub handle: String,
    pub first_name: String,
    pub last_name: String,
    pub primary_email: String,
    pub is_active: bool,
    pub is_staff: bool,
    pub is_superuser: bool,
    pub is_page_manager: bool,
    pub verified: bool,
    pub date_joined: String,
    pub emails: Vec<UserEmail>,
}

impl AdminUser {
    /// Returns the display name, preferring first/last name over handle
    pub fn display_name(&self) -> String {
        if self.first_name.is_empty() && self.last_name.is_empty() {
            self.handle.clone()
        } else {
            format!("{} {}", self.first_name, self.last_name)
                .trim()
                .to_string()
        }
    }

    /// Returns the user role as a display string
    /// Note: superuser is displayed as "staff" in the UI
    pub fn display_role(&self) -> &'static str {
        if self.is_superuser || self.is_staff {
            "staff"
        } else if self.is_page_manager {
            "page_manager"
        } else {
            "user"
        }
    }

    /// Returns the user role for form values (distinguishes superuser from staff)
    pub fn form_role(&self) -> &'static str {
        if self.is_superuser {
            "superuser"
        } else if self.is_staff {
            "staff"
        } else if self.is_page_manager {
            "page_manager"
        } else {
            "user"
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminUserListResponse {
    pub users: Vec<AdminUser>,
    pub total_count: u64,
    pub page: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminUserSearchRequest {
    pub search: Option<String>,
    pub role: Option<String>,         // user, page_manager, staff
    pub email_status: Option<String>, // verified, unverified
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminUserCreateRequest {
    pub handle: String,
    pub first_name: String,
    pub last_name: String,
    pub primary_email: String,
    pub password: String,
    pub role: String, // user, page_manager, staff
    pub verified: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminUserUpdateRequest {
    pub user_id: Ulid,
    pub handle: String,
    pub first_name: String,
    pub last_name: String,
    pub primary_email: String,
    pub new_password: Option<String>, // None means don't change password
    pub role: String,                 // user, page_manager, staff
    pub verified: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminUserDeleteRequest {
    pub user_id: Ulid,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminUserResponse {
    pub user: AdminUser,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminUserDeleteResponse {
    pub success: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminUserGetRequest {
    pub user_id: Ulid,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminUserBatchGetRequest {
    pub user_ids: Vec<Ulid>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserNameMapping {
    pub user_id: Ulid,
    pub primary_email: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminUserBatchGetResponse {
    pub users: Vec<UserNameMapping>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageManagerSyncRequest {
    pub user_ids: Vec<Ulid>, // All user IDs that should be page managers
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageManagerSyncResponse {
    pub updated_count: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageManagerUpdateRequest {
    pub user_id: Ulid,
    pub is_page_manager: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageManagerUpdateResponse {
    pub success: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminPasswordResetRequest {
    pub user_id: Ulid,
    pub email: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdminPasswordResetResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserEmailsRequest {
    pub user_id: Ulid,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserEmail {
    pub email: String,
    pub verified: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserEmailsResponse {
    pub emails: Vec<UserEmail>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserEmailAddRequest {
    pub user_id: Ulid,
    pub email: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserEmailUpdateRequest {
    pub user_id: Ulid,
    pub old_email: String,
    pub new_email: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserEmailDeleteRequest {
    pub user_id: Ulid,
    pub email: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserEmailToggleVerifiedRequest {
    pub user_id: Ulid,
    pub email: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserEmailResponse {
    pub success: bool,
    pub message: String,
    pub emails: Vec<UserEmail>, // Return updated emails list
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct AdminAnalyticsRequest {
    // Empty for now - could add date ranges later
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct AdminAnalyticsResponse {
    pub total_users: u32,
    pub verified_users: u32,
    pub monthly_signups: u32,
    pub weekly_signups: u32,
    pub daily_signups: u32,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct GeographicAnalyticsRequest {
    pub start_date: String, // Start date in YYYY-MM-DD format
    pub end_date: String,   // End date in YYYY-MM-DD format
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct GeographicStat {
    pub country: String,
    pub country_code: String,
    pub user_count: i64,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct AdminUserExportRequest {
    pub start_date: String, // Start date in YYYY-MM-DD format
    pub end_date: String,   // End date in YYYY-MM-DD format
    pub page: Option<u32>,  // Page number (1-indexed), for pagination
    pub limit: Option<u32>, // Number of users per page
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct AdminUserExportResponse {
    pub users: Vec<AdminUser>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct GeographicAnalyticsResponse {
    pub stats: Vec<GeographicStat>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ActivityAnalyticsRequest {
    pub start_date: String, // Start date in YYYY-MM-DD format
    pub end_date: String,   // End date in YYYY-MM-DD format
    pub page: u32,          // Page number (1-indexed)
    pub per_page: u32,      // Items per page
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ActivityStat {
    pub date: String,
    pub new_users: i64,
    pub total_users_to_date: i64,
    pub verified_users_to_date: i64,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ActivityAnalyticsResponse {
    pub stats: Vec<ActivityStat>,
    pub total_count: usize, // Total number of days in date range
}

// Address Sets
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressSetInfo {
    pub slug: String,
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressSetListRequest {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressSetListResponse {
    pub address_sets: Vec<AddressSetInfo>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressSetGetRequest {
    pub slug: String,
    pub page: Option<u32>,  // Page number (0-indexed), None = return all
    pub limit: Option<u32>, // Items per page, None = return all
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressSetGetResponse {
    pub slug: String,
    pub name: String,
    pub addresses: Vec<String>,
    pub total_count: u64,    // Total number of addresses in the set
    pub has_more: bool,       // Whether there are more pages available
}

// User Sets
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetInfo {
    pub slug: String,
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetListRequest {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetListResponse {
    pub user_sets: Vec<UserSetInfo>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetGetRequest {
    pub slug: String,
    pub page: Option<u32>,  // Page number (0-indexed), None = return all
    pub limit: Option<u32>, // Items per page, None = return all
    pub evaluation_id: Option<String>, // If provided, use existing result set; if not, compute new one
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetGetResponse {
    pub slug: String,
    pub name: String,
    pub user_ids: Vec<Ulid>,
    pub total_count: u64,    // Total number of users in the set
    pub has_more: bool,       // Whether there are more pages available
    pub evaluation_id: String, // Identifier for this result set, use to release it when done
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetReleaseResultsRequest {
    pub evaluation_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetReleaseResultsResponse {
    pub success: bool,
}

// User Set Criteria
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, strum::EnumDiscriminants)]
#[strum_discriminants(derive(strum::EnumString, strum::AsRefStr))]
#[strum_discriminants(name(UserSetCriterionType))]
pub enum UserSetCriterion {
    HasTag { tag: String },
    HasPointInRegion { geojson: String },
    AffiliateParticipation {
        affiliate_id: Ulid,
        start: TimeString,
        end: TimeString
    },
    IsPageManager,
    IsStaff,
    IsVerified,
    ExplicitList { user_ids: Vec<Ulid> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, strum::EnumString, strum::AsRefStr)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum CriterionOperation {
    Union,
    Intersect,
}

impl Default for CriterionOperation {
    fn default() -> Self {
        CriterionOperation::Union
    }
}

// Address Set CRUD
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressSetCreateRequest {
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressSetUpdateRequest {
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressSetDeleteRequest {
    pub slug: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressSetResponse {
    pub success: bool,
    pub message: String,
}

// Address Set Member Management
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressSetAddMemberRequest {
    pub slug: String,
    pub email_address: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressSetRemoveMemberRequest {
    pub slug: String,
    pub email_address: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressSetMemberResponse {
    pub success: bool,
    pub message: String,
}

// User Set CRUD
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetCreateRequest {
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetUpdateRequest {
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetDeleteRequest {
    pub slug: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetResponse {
    pub success: bool,
    pub message: String,
}

// User Set Criteria Management
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetAddCriterionRequest {
    pub slug: String,
    pub criterion: UserSetCriterion,
    pub operation: CriterionOperation,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetRemoveCriterionRequest {
    pub slug: String,
    pub criterion_id: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetCriterionResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetCriterionWithId {
    pub id: i64,
    pub criterion: UserSetCriterion,
    #[serde(default)]
    pub operation: CriterionOperation,
}

impl UserSetCriterionWithId {
    pub fn display_type(&self) -> &'static str {
        match &self.criterion {
            UserSetCriterion::HasTag { .. } => "HasTag",
            UserSetCriterion::HasPointInRegion { .. } => "HasPointInRegion",
            UserSetCriterion::AffiliateParticipation { .. } => "AffiliateParticipation",
            UserSetCriterion::IsPageManager => "IsPageManager",
            UserSetCriterion::IsStaff => "IsStaff",
            UserSetCriterion::IsVerified => "IsVerified",
            UserSetCriterion::ExplicitList { .. } => "ExplicitList",
        }
    }

    pub fn display_operation(&self) -> &'static str {
        match self.operation {
            CriterionOperation::Union => "OR",
            CriterionOperation::Intersect => "AND",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserSetDetailResponse {
    pub slug: String,
    pub name: String,
    pub criteria: Vec<UserSetCriterionWithId>,
}

// User Info for Email Sending (lightweight alternative to AdminUser)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserEmailInfoRequest {
    pub user_id: Ulid,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserEmailInfo {
    pub user_id: Ulid,
    pub handle: String,
    pub first_name: String,
    pub last_name: String,
    pub primary_email: String,
    pub verified: bool,
}

// Recipient Search (for email sending UI)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecipientSearchRequest {
    pub query: String,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecipientSearchResponse {
    pub results: Vec<RecipientSearchResult>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum RecipientSearchResult {
    Email {
        email: String,
    },
    User {
        user_id: String,
        display_name: String,
        email: String,
    },
    UserSet {
        slug: String,
        name: String,
    },
    AddressSet {
        slug: String,
        name: String,
    },
}
