use std::collections::BTreeMap;

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
            format!("{} {}", self.first_name, self.last_name).trim().to_string()
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
    pub role: Option<String>, // user, page_manager, staff
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
    pub role: String, // user, page_manager, staff
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
