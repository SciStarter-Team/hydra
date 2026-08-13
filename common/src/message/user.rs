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
    /// A badge staked out by the tracked go link the visitor arrived through
    /// (orrery's `grant-badge` session key). Carried on the signup rather than
    /// granted by the caller so that both lanes obey it: the immediate one,
    /// and the deferred one, where it waits on the pending row until the
    /// emailed link is clicked — possibly in another browser, where no session
    /// key survives.
    pub badgeclass: Option<crate::badge::Badge>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Login {
    pub email: String,
    pub password: String,
}

/// A signup that Turnstile could not vouch for, to be deferred behind an
/// emailed completion link instead of being rejected. Payload for
/// `user.auth.defer_signup`.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct DeferSignup {
    pub signup: Signup,
    /// Local path to resume once the account is created.
    pub next: Option<String>,
    /// Client IP, used to rate limit the deferred lane.
    pub ip_address: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub enum DeferSignupOutcome {
    /// The completion email was sent, or one was already sent recently.
    Deferred,
    /// The address already belongs to an account; the visitor should log in.
    AlreadyRegistered,
    /// Too many deferred signups from this IP address in the last hour.
    RateLimited,
}

/// Payload for `user.auth.complete_signup`: redeem an emailed completion
/// code, creating the deferred account. The response is None when the code
/// is unknown or expired.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct CompleteSignup {
    pub code: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct CompleteSignupResponse {
    pub basic_info: BasicInfo,
    /// Local path to resume, captured when the signup was deferred.
    pub next: Option<String>,
}

/// Profile fields returned to Science Near Me after a successful credential
/// check (the encrypted payload of the /api/login-for-snm bridge).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct SnmProfile {
    /// The user's handle, or None when unset.
    pub username: Option<String>,
    pub first_name: String,
    pub last_name: String,
    /// All of the user's email addresses (primary first).
    pub emails: Vec<String>,
    /// The user's zip code, or None when unset.
    pub zip_code: Option<String>,
    pub newsletter: bool,
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
    fn has_handle(&self) -> bool;

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

    fn has_handle(&self) -> bool {
        false
    }
}

/// What a private user is called wherever their identity would otherwise
/// appear. Every surface that names a user to somebody else must render a
/// private user as this, never as their handle: handles are frequently
/// district usernames or real names.
pub const PRIVATE_DISPLAY_NAME: &str = "Private";

/// Why a protected user's message is refused, in the words shown to them.
/// Shared so the frontend can say it before the send and forum-service can
/// say it while refusing one that arrives anyway.
pub const PROTECTED_SENDER_REFUSAL: &str = "As a side effect of protecting your privacy, it is impossible for you to send messages via SciStarter.";

/// A tag name in the one form both codebases store, matching what orrery's
/// `Tags` does on the way into its column: stripped of surrounding whitespace
/// and of the `#` people write when they mean a hashtag, then lowercased.
///
/// Every path that writes a tag, and every rule that recognizes one, goes
/// through here. A tag stored in some other spelling would be a row orrery
/// could never have produced; a tag *matched* in some other spelling silently
/// fails to be the tag it plainly is, which is how `#girlscout` once arrived
/// on an account without protecting it.
///
/// Returns the empty string for anything that was only punctuation and space;
/// callers drop those rather than store a nameless tag.
pub fn normalize_tag(tag: &str) -> String {
    tag.trim().trim_matches('#').to_lowercase()
}

/// Signup-context tags (from a `?tag=` parameter, carried in the session)
/// that mark the account being created as one whose owner needs protection
/// — a cohort signing up through a partner that enrolls minors. Kept in
/// step with orrery's `PROTECTED_SIGNUP_TAGS`.
pub const PROTECTED_SIGNUP_TAGS: [&str; 2] = ["laplstudent", "girlscout"];

/// Whether an account signing up under `tags` must be protected.
///
/// Matched on the normalized name rather than the literal one: the tag reaches
/// us from URLs typed by partners, and only some of the paths it travels
/// lowercase it or drop a leading `#` on the way. What is at stake in a missed
/// match is a minor's account going unprotected, so this is generous about
/// spelling in the one direction that can only ever protect more people.
pub fn signup_needs_protection<S: AsRef<str>>(tags: &[S]) -> bool {
    tags.iter().any(|tag| {
        let tag = normalize_tag(tag.as_ref());

        PROTECTED_SIGNUP_TAGS.contains(&tag.as_str())
    })
}

/// Signup-context tags whose cohort is credited with the Foundations of
/// Citizen Science module rather than working through it on the site. The
/// library runs the module with its members before enrolling them, so the
/// grant records something that already happened off-site. Kept in step with
/// orrery's `FOUNDATIONS_GRANT_SIGNUP_TAGS`.
///
/// Kept apart from `PROTECTED_SIGNUP_TAGS` even though the membership
/// overlaps: protecting an account and crediting it are separate decisions
/// about the same cohort, and a partner can need one without the other --
/// girlscout is protected and is not credited, ldpl is credited and is not
/// protected.
pub const FOUNDATIONS_GRANT_SIGNUP_TAGS: [&str; 2] = ["laplstudent", "ldpl"];

/// Whether an account carrying `tags` from its signup context is credited
/// with the Foundations module outright.
pub fn signup_grants_foundations<S: AsRef<str>>(tags: &[S]) -> bool {
    tags.iter().any(|tag| {
        let tag = normalize_tag(tag.as_ref());

        FOUNDATIONS_GRANT_SIGNUP_TAGS.contains(&tag.as_str())
    })
}

impl BasicInfo {
    pub fn is_private(&self) -> bool {
        !self.public || self.protected
    }

    pub fn display_handle(&self) -> &str {
        if self.is_private() {
            PRIVATE_DISPLAY_NAME
        } else if self.handle.trim().is_empty() {
            "Nemo" // Maybe this should just be "No Handle", "Nemo" is kind of quirky
        } else {
            &self.handle
        }
    }

    /// Whether this user may originate a message. A protected user may not:
    /// the message would name them to whoever receives it, which is the one
    /// thing their protection exists to prevent. Merely non-public users are
    /// unaffected — they chose a quiet profile, not anonymity.
    ///
    /// orrery allows protected users one exception, writing to the
    /// administrator or manager of an edtech group they belong to; hydra has
    /// no group membership to hang that exception on, so it refuses outright.
    pub fn may_send_messages(&self) -> bool {
        !self.protected
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

    fn has_handle(&self) -> bool {
        !self.handle.is_empty()
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

    fn has_handle(&self) -> bool {
        match self {
            Some(info) => info.has_handle(),
            None => Unauthenticated.has_handle(),
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

    fn has_handle(&self) -> bool {
        match self {
            Some(info) => info.has_handle(),
            None => Unauthenticated.has_handle(),
        }
    }
}

impl UserInfo for Option<Arc<BasicInfo>> {
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

    fn has_handle(&self) -> bool {
        match self {
            Some(info) => info.has_handle(),
            None => Unauthenticated.has_handle(),
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
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserAndBadge {
    pub user_id: Ulid,
    pub badge: Badge,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BadgeHoldersRequest {
    pub badge: Badge,
    pub search: Option<String>,
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BadgeHolderInfo {
    pub user_id: Ulid,
    pub handle: String,
    pub first_name: String,
    pub last_name: String,
    pub primary_email: String,
    pub verified: bool,
    pub when: TimeString,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BadgeHoldersResponse {
    pub holders: Vec<BadgeHolderInfo>,
    pub total_count: u32,
    pub page: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserCountResponse {
    pub count: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDetailsListRequest {
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDetails {
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
    pub orcid: Option<String>,
}

impl UserDetails {
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
pub struct UserDetailsListResponse {
    pub users: Vec<UserDetails>,
    pub total_count: u64,
    pub page: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDetailsSearchRequest {
    pub search: Option<String>,
    pub role: Option<String>,         // user, page_manager, staff
    pub email_status: Option<String>, // verified, unverified
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDetailsCreateRequest {
    pub handle: String,
    pub first_name: String,
    pub last_name: String,
    pub primary_email: String,
    pub password: String,
    pub role: String, // user, page_manager, staff
    pub verified: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDetailsUpdateRequest {
    pub user_id: Ulid,
    pub handle: String,
    pub first_name: String,
    pub last_name: String,
    pub primary_email: String,
    pub new_password: Option<String>, // None means don't change password
    pub role: String,                 // user, page_manager, staff
    pub verified: bool,
    pub is_active: bool,
    pub orcid: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDetailsDeleteRequest {
    pub user_id: Ulid,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDetailsResponse {
    pub user: UserDetails,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDetailsDeleteResponse {
    pub success: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDetailsGetRequest {
    pub user_id: Ulid,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDetailsBatchGetRequest {
    pub user_ids: Vec<Ulid>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserNameMapping {
    pub user_id: Ulid,
    pub primary_email: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDetailsBatchGetResponse {
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
pub struct UserDetailsExportRequest {
    pub start_date: String, // Start date in YYYY-MM-DD format
    pub end_date: String,   // End date in YYYY-MM-DD format
    pub page: Option<u32>,  // Page number (1-indexed), for pagination
    pub limit: Option<u32>, // Number of users per page
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct UserDetailsExportResponse {
    pub users: Vec<UserDetails>,
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
    pub total_count: u64, // Total number of addresses in the set
    pub has_more: bool,   // Whether there are more pages available
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
    pub total_count: u64,      // Total number of users in the set
    pub has_more: bool,        // Whether there are more pages available
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
    HasTag {
        tag: String,
    },
    HasPointInRegion {
        geojson: String,
    },
    AffiliateParticipation {
        affiliate_id: Ulid,
        start: TimeString,
        end: TimeString,
    },
    IsPageManager,
    IsStaff,
    IsVerified,
    ExplicitList {
        user_ids: Vec<Ulid>,
    },
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    strum::EnumString,
    strum::AsRefStr,
)]
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

// User Display Info (for public display in reviews, forums, etc.)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDisplayInfoRequest {
    pub user_id: Ulid,
}

/// Display identity for a user who is being shown to somebody else. The
/// producer has already redacted `handle` and `image` for a private user,
/// so these fields are always safe to render; `private` is there so a
/// caller can go further and withhold the content itself, the way public
/// reviews do.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDisplayInfo {
    pub user_id: Ulid,
    pub handle: String,
    pub image: String,
    pub private: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDisplayInfoBatchRequest {
    pub user_ids: Vec<Ulid>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserDisplayInfoBatchResponse {
    pub users: Vec<UserDisplayInfo>,
}

// User Info for Email Sending (lightweight alternative to UserDetails)
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
    pub accept_emails: bool,
}

// User lookup by email address
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserByEmailRequest {
    pub email: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserByEmailResponse {
    pub found: bool,
    pub user_id: Option<String>,
    pub handle: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
}

// Recipient Search (for email sending UI)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum RecipientFilter {
    Users,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecipientSearchRequest {
    pub query: String,
    pub limit: Option<u32>,
    pub filter: Option<RecipientFilter>,
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

// Bookmarks
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BookmarkCreateRequest {
    pub user_id: Ulid,
    pub project_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BookmarkCreateResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BookmarkCheckRequest {
    pub user_id: Ulid,
    pub project_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BookmarkCheckResponse {
    pub has_bookmark: bool,
}

// Project joins
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectJoinRequest {
    pub user_id: Ulid,
    pub project_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectJoinResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectJoinCheckRequest {
    pub user_id: Ulid,
    pub project_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectJoinCheckResponse {
    pub has_joined: bool,
}

/// Batch request for per-entity engagement counts. Used by
/// `user.joins.count_by_projects` and `user.bookmarks.count_by_projects`
/// to decorate the dashboard's owned-entity listings.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntityCountsRequest {
    pub entity_ids: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntityCountsResponse {
    /// One `(entity_id, count)` pair per requested id that has a
    /// nonzero count; ids with no rows are omitted.
    pub counts: Vec<(String, i64)>,
}

// List all project joins for a user (for API check-join endpoint)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserJoinsListRequest {
    pub opaque_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserJoinsListResponse {
    pub found: bool,
    pub projects: Vec<UserJoinEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserJoinEntry {
    pub project_id: String,
}

// List all users who opted out of AI
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiOptedOutRequest {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiOptedOutResponse {
    pub opted_out: Vec<AiOptedOutEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiOptedOutEntry {
    pub opaque_id: String,
    pub opted_out_at: Option<String>,
}

// Permissions
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermissionCheckRequest {
    pub user_id: String, // ULID
    pub resource_type: String,
    pub resource_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermissionCheckResponse {
    pub has_permission: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermissionGrantRequest {
    pub user_id: String,
    pub resource_type: String,
    pub resource_id: String,
    pub permission: String,
    pub granted_by: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermissionRevokeRequest {
    pub user_id: String,
    pub resource_type: String,
    pub resource_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermissionListRequest {
    pub user_id: String,
    pub resource_type: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Permission {
    pub user_id: String,
    pub resource_type: String,
    pub resource_id: String,
    pub permission: String,
    pub granted_by: String,
    pub granted_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermissionListResponse {
    pub permissions: Vec<Permission>,
}

// ============================================================================
// User Matching (for API participation recording)
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum IdentifierType {
    Hashed,    // SHA256 hash of email
    Opaque,    // User's opaque_id UUID
    Email,     // Plaintext email
    LinkToken, // Link token string
    Ulid,      // User's ULID (hydra user_id == orrery User.ulid)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserMatchRequest {
    pub identifier: String,
    pub identifier_type: IdentifierType,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserMatchResponse {
    pub user_id: Option<String>, // ULID, None = no match (discard for privacy)
}

// ============================================================================
// Legacy Profile ID Lookup (for v1 API compat)
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProfileIdLookupRequest {
    pub identifier: String,
    pub identifier_type: ProfileIdLookupType,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ProfileIdLookupType {
    Email,
    Hashed,
    LegacyProfileId,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProfileIdLookupResponse {
    pub user_id: Option<String>,
    pub legacy_profile_id: Option<i64>,
    pub opaque_id: Option<String>,
    pub known: bool,
    /// Whether the user permits their contributions to be recorded. Defaults to
    /// true (and is meaningless when `known` is false).
    #[serde(default = "crate::message::default_true")]
    pub track_contributions: bool,
}

// ============================================================================
// Training-state export (for /api/training-state.json)
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrainingStatesRequest {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FoundationsStateEntry {
    /// The user's opaque id.
    pub person: String,
    /// Derived Foundations state name (e.g. "hour1", "squirrel", "bookmark2",
    /// "badge"); only users past the initial state are included.
    pub state: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrainingStatesResponse {
    pub states: Vec<FoundationsStateEntry>,
}

// ============================================================================
// Ambassador map (for /api/ambassador_map.geojson)
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AmbassadorPassedLocationsRequest {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AmbassadorLocation {
    pub city: String,
    pub state: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AmbassadorPassedLocationsResponse {
    pub locations: Vec<AmbassadorLocation>,
}

// ============================================================================
// API User Creation (for v1 API compat)
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiCreateUserRequest {
    pub email: String,
    pub handle: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub zipcode: Option<String>,
    pub password: Option<String>,
    /// Account origin (the v1 API sets "API"). Applied to newly-created users.
    #[serde(default)]
    pub origin: Option<String>,
    /// Referring project slug, recorded on newly-created users.
    #[serde(default)]
    pub referer: Option<String>,
    /// Opaque caller-supplied user-type code, stored as misc `{"user_type": N}`.
    #[serde(default)]
    pub user_type: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiCreateUserResponse {
    pub user_id: String,
    pub legacy_profile_id: Option<i64>,
    pub opaque_id: Option<String>,
    pub already_existed: bool,
}

// ============================================================================
// API Key Management
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserApiKeyRequest {
    pub user_id: String, // ULID
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserApiKeyResponse {
    pub api_key: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserOpaqueIdRequest {
    pub user_id: String, // ULID
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserOpaqueIdResponse {
    pub opaque_id: Option<String>, // UUID string, None if not set
}

// ============================================================================
// API Key Validation
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValidateApiKeyRequest {
    pub api_key: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValidateApiKeyResponse {
    pub valid: bool,
    pub user_id: Option<String>, // ULID if valid
    pub is_staff: bool,
    pub is_superuser: bool,
    /// Whether the key's user is flagged to auto-affiliate the projects they
    /// create via the API. Defaults to false.
    #[serde(default)]
    pub automatic_affiliate: bool,
}

// ============================================================================
// Articulate Workflow Progress
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkflowProgressRequest {
    pub user_id: String, // ULID
    pub workflow: WorkflowType,
    pub step: String, // e.g., "hour1", "tutorial", "squirrel"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkflowProgressResponse {
    pub success: bool,
    pub badge_granted: bool, // True if this step completion triggered a badge
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WorkflowType {
    Foundations,
    LibrariesHubs,
    DataLiteracy,
    HigherEd,
    DataEthics,
    Ideal,
    Ambassador,
    K12,
}

impl WorkflowType {
    /// Stable identifier used as the per-training SSE event name (and in URLs).
    /// Both the user-service (which publishes progress events) and the frontend
    /// training pages (which listen for them) must agree on these.
    pub fn slug(self) -> &'static str {
        match self {
            WorkflowType::Foundations => "foundations",
            WorkflowType::LibrariesHubs => "libraries-hubs",
            WorkflowType::DataLiteracy => "data-literacy",
            WorkflowType::HigherEd => "higher-ed",
            WorkflowType::DataEthics => "data-ethics",
            WorkflowType::Ideal => "ideal",
            WorkflowType::Ambassador => "ambassador",
            WorkflowType::K12 => "k12",
        }
    }

    /// Resolve orrery's `CountedRedirect.statemachine` spelling — the
    /// lowercased Django model class name — to a workflow. Tracked go links
    /// carry these, so a visit can advance a training; the accompanying
    /// `transition` is the step name, which both sides already spell the same
    /// way ("tutorial", "list", …).
    pub fn from_statemachine(name: &str) -> Option<WorkflowType> {
        match name.to_ascii_lowercase().as_str() {
            "foundationsstate" => Some(WorkflowType::Foundations),
            "librarieshubsstate" => Some(WorkflowType::LibrariesHubs),
            "dataliteracystate" => Some(WorkflowType::DataLiteracy),
            "higheredstate" => Some(WorkflowType::HigherEd),
            "dataethicsstate" => Some(WorkflowType::DataEthics),
            "idealstate" => Some(WorkflowType::Ideal),
            "ambassadorstate" => Some(WorkflowType::Ambassador),
            "k12state" => Some(WorkflowType::K12),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkflowStatusRequest {
    pub user_id: String, // ULID
    pub workflow: WorkflowType,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkflowStatusResponse {
    pub current_step: Option<String>, // Current step identifier, None if not started
    pub completed_steps: Vec<String>, // All completed steps
    pub badge_granted: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkflowTutorialCountRequest {
    pub workflow: WorkflowType,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkflowTutorialCountResponse {
    pub count: i64, // Platform-wide tally of completed tutorials for this workflow
}

/// One stage in a training funnel: the number of users who reached it. Stages
/// are ordered start -> finish, so counts are monotonically non-increasing.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FunnelStage {
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkflowFunnel {
    pub name: String,
    pub stages: Vec<FunnelStage>,
    /// Optional clarification (e.g. when steps are alternatives, not a sequence).
    pub note: Option<String>,
}

/// Platform-wide engagement funnel for every training workflow (the
/// training-analytics staff report).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkflowFunnelResponse {
    pub funnels: Vec<WorkflowFunnel>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermissionListByResourceRequest {
    pub resource_type: String,
    pub resource_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermissionListByResourceResponse {
    pub permissions: Vec<Permission>,
}

// ============================================================================
// Communities
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityInfo {
    pub community_id: Ulid,
    pub name: String,
    pub slug: String,
    pub subtitle: String,
    pub intro: String,
    pub description: String,
    pub image_url: String,
    pub partner_tag: String,
    pub topics: Vec<String>,
    pub external_url: String,
    pub active: bool,
    pub featured: bool,
    pub exclusive: bool,
    pub sort_order: i32,
    pub member_count: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityListRequest {
    pub active_only: bool,
    pub featured_only: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityListResponse {
    pub communities: Vec<CommunityInfo>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityGetRequest {
    pub slug: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityCreateRequest {
    pub name: String,
    pub slug: String,
    pub subtitle: String,
    pub intro: String,
    pub description: String,
    pub image_url: String,
    pub partner_tag: String,
    pub topics: Vec<String>,
    pub external_url: String,
    pub active: bool,
    pub featured: bool,
    pub exclusive: bool,
    pub sort_order: i32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityUpdateRequest {
    pub community_id: Ulid,
    pub name: String,
    pub slug: String,
    pub subtitle: String,
    pub intro: String,
    pub description: String,
    pub image_url: String,
    pub partner_tag: String,
    pub topics: Vec<String>,
    pub external_url: String,
    pub active: bool,
    pub featured: bool,
    pub exclusive: bool,
    pub sort_order: i32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityDeleteRequest {
    pub community_id: Ulid,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityJoinRequest {
    pub community_id: Ulid,
    pub user_id: Ulid,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityLeaveRequest {
    pub community_id: Ulid,
    pub user_id: Ulid,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityMemberCheckRequest {
    pub community_id: Ulid,
    pub user_id: Ulid,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityMemberCheckResponse {
    pub is_member: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserCommunitiesRequest {
    pub user_id: Ulid,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserCommunitiesResponse {
    pub communities: Vec<CommunityInfo>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunitySuccessResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityMembership {
    pub user_id: Ulid,
    pub handle: String,
    pub first_name: String,
    pub last_name: String,
    pub primary_email: String,
    pub joined_at: String,
}

impl CommunityMembership {
    pub fn display_name(&self) -> String {
        if self.first_name.is_empty() && self.last_name.is_empty() {
            self.handle.clone()
        } else {
            format!("{} {}", self.first_name, self.last_name)
                .trim()
                .to_string()
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityMembersRequest {
    pub community_id: Ulid,
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunityMembersResponse {
    pub members: Vec<CommunityMembership>,
    pub total_count: u64,
    pub page: u32,
    pub limit: u32,
}

// ============================================================================
// Certificates
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CertificateCreateRequest {
    pub user_id: Ulid,
    pub project: String,
    pub mode: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CertificateCreateResponse {
    pub certificate_id: Ulid,
    pub already_existed: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CertificateCheckRequest {
    pub user_id: Ulid,
    pub project: String,
    pub mode: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CertificateCheckResponse {
    pub exists: bool,
    pub certificate_id: Option<Ulid>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserCertificatesRequest {
    pub user_id: Ulid,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserCertificatesResponse {
    pub certificates: Vec<CertificateInfo>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CertificateInfo {
    pub certificate_id: Ulid,
    pub created_at: String,
    pub user_id: Ulid,
    pub project: String,
    pub mode: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CertificateGetRequest {
    pub certificate_id: Ulid,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CertificateGetResponse {
    pub certificate_id: Ulid,
    pub created_at: String,
    pub user_id: Ulid,
    pub project: String,
    pub mode: String,
    pub display_name: String,
}

// ============================================================================
// iNaturalist
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InatListUsersRequest {
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InatUser {
    pub user_id: Ulid,
    pub inat_username: String,
    pub inat_last_update: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InatUpdateLastSync {
    pub user_id: Ulid,
    pub inat_last_update: String,
}

// ============================================================================
// Legacy user-id lookup
// ============================================================================

/// Look up a hydra user_id by orrery's User-level legacy id. Distinct
/// from `user.profile.lookup` which deals with Profile-level legacy
/// ids (`legacy_v*_profile_id`). This handler only inspects
/// `legacy_v3_id` and `legacy_v2_id`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserLegacyIdLookupRequest {
    pub legacy_id: i32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserLegacyIdLookupResponse {
    pub user_id: Option<Ulid>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic_info(public: bool, protected: bool) -> BasicInfo {
        BasicInfo {
            user_id: Ulid::nil(),
            handle: "ada-district-1234".to_string(),
            is_staff: false,
            is_superuser: false,
            public,
            protected,
            needs_point: false,
        }
    }

    #[test]
    fn a_protected_user_is_never_named_to_anybody() {
        let protected = basic_info(true, true);

        assert!(protected.is_private());
        assert_eq!(protected.display_handle(), PRIVATE_DISPLAY_NAME);
    }

    #[test]
    fn a_quiet_profile_is_also_unnamed() {
        assert_eq!(
            basic_info(false, false).display_handle(),
            PRIVATE_DISPLAY_NAME
        );
    }

    #[test]
    fn a_public_user_is_named_by_their_handle() {
        let public = basic_info(true, false);

        assert!(!public.is_private());
        assert_eq!(public.display_handle(), "ada-district-1234");
    }

    #[test]
    fn the_protecting_signup_tags_are_recognized() {
        assert!(signup_needs_protection(&["girlscout".to_string()]));
        assert!(signup_needs_protection(&["laplstudent".to_string()]));
        // The tag arrives from partner-authored URLs, in whatever case they
        // wrote it, and only some paths lowercase it on the way here.
        assert!(signup_needs_protection(&["LAPLstudent".to_string()]));
        assert!(signup_needs_protection(&[" GirlScout ".to_string()]));
        // And in whatever punctuation: a partner writing the tag as a hashtag
        // is writing the same tag, and a minor's account turning on that is
        // not a trade worth making.
        assert!(signup_needs_protection(&["#girlscout".to_string()]));
        assert!(signup_needs_protection(&[" #GirlScout ".to_string()]));
        assert!(signup_needs_protection(&[
            "some-partner".to_string(),
            "girlscout".to_string(),
        ]));
    }

    /// The one spelling everything that writes or matches a tag agrees on.
    ///
    /// Kept in step with orrery's `normalize_tag`, and depended on by more
    /// than the mirror: a name written one way and matched another is how a
    /// protecting tag lands on an account without protecting it.
    #[test]
    fn a_tag_has_one_spelling() {
        assert_eq!(normalize_tag(" #GirlScout "), "girlscout");
        assert_eq!(normalize_tag("their-campaign"), "their-campaign");
        assert_eq!(normalize_tag("#a"), "a");

        // Punctuation and space is not a tag at all.
        assert_eq!(normalize_tag(" ## "), "");
        assert_eq!(normalize_tag(""), "");

        // Idempotent, so a name that has been through here once can go through
        // again -- which it does, every time a stored tag is matched.
        for raw in [" #GirlScout ", "their-campaign", "#a", " ## "] {
            assert_eq!(normalize_tag(&normalize_tag(raw)), normalize_tag(raw));
        }
    }

    #[test]
    fn an_ordinary_signup_is_not_protected() {
        assert!(!signup_needs_protection(&["some-partner".to_string()]));
        assert!(!signup_needs_protection(&["girlscouts".to_string()]));
        assert!(!signup_needs_protection::<String>(&[]));
    }

    #[test]
    fn the_crediting_signup_tags_are_recognized() {
        assert!(signup_grants_foundations(&["laplstudent".to_string()]));
        assert!(signup_grants_foundations(&["ldpl".to_string()]));
        // Handed out as `/login?tag=LDPL`, and arriving in whatever case and
        // punctuation the partner wrote it in.
        assert!(signup_grants_foundations(&["LDPL".to_string()]));
        assert!(signup_grants_foundations(&[" #LDPL ".to_string()]));
        assert!(signup_grants_foundations(&[
            "some-partner".to_string(),
            "LAPLstudent".to_string(),
        ]));
    }

    /// The two rules name overlapping cohorts and decide separate things, in
    /// both directions. Pinned because the sets are easy to mistake for one
    /// another, and either confusion is a real harm: crediting a badge to a
    /// cohort that did not earn it, or leaving a minor's account unprotected.
    #[test]
    fn crediting_and_protecting_are_separate_decisions() {
        assert!(signup_needs_protection(&["girlscout".to_string()]));
        assert!(!signup_grants_foundations(&["girlscout".to_string()]));

        assert!(signup_grants_foundations(&["ldpl".to_string()]));
        assert!(!signup_needs_protection(&["ldpl".to_string()]));
    }

    #[test]
    fn an_ordinary_signup_is_not_credited() {
        assert!(!signup_grants_foundations(&["some-partner".to_string()]));
        assert!(!signup_grants_foundations(&["ldpls".to_string()]));
        assert!(!signup_grants_foundations::<String>(&[]));
    }

    #[test]
    fn only_protection_bars_sending() {
        assert!(!basic_info(true, true).may_send_messages());
        assert!(!basic_info(false, true).may_send_messages());
        assert!(basic_info(false, false).may_send_messages());
        assert!(basic_info(true, false).may_send_messages());
    }
}
