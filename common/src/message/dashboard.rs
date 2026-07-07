use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// Full self-readable view of a user's profile. Returned by
/// `user.profile.get` for the user themselves or for staff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub user_id: Ulid,
    pub handle: String,
    pub first_name: String,
    pub last_name: String,
    pub primary_email: String,
    pub image: Option<String>,
    pub bio: Option<String>,
    pub birthday: Option<String>,
    pub gender: Option<String>,
    pub zipcode: Option<String>,
    pub country: Option<String>,
    pub country_code: String,
    pub mailing_address: Option<String>,
    pub facebook_url: Option<String>,
    pub twitter_profile: Option<String>,
    pub home_page: Option<String>,
    pub inat_username: Option<String>,
    pub zooniverse_username: String,
    pub orcid: Option<String>,
    pub degree: Option<String>,
    pub ethnicity: Option<String>,
    pub income: Option<String>,
    pub scientist: bool,
    pub educator: bool,
    pub ambassador: Option<String>,
    pub public: bool,
    pub protected: bool,
    pub accept_emails: bool,
    pub newsletter: bool,
    pub accept_messages: bool,
    pub track_contributions: bool,
    pub ai_opt_out: bool,
    pub date_joined: String,
    pub point: Option<String>,
    pub extra_skills: String,
    pub extra_tools: String,
}

/// Fields that the user is allowed to update on their own profile.
/// Sent via `user.profile.save`. Fields wrapped in `Option` are only
/// updated when `Some(_)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfileUpdate {
    pub user_id: Ulid,
    pub handle: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub image: Option<String>,
    pub bio: Option<String>,
    pub birthday: Option<String>,
    pub gender: Option<String>,
    pub zipcode: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub mailing_address: Option<String>,
    pub facebook_url: Option<String>,
    pub twitter_profile: Option<String>,
    pub home_page: Option<String>,
    pub inat_username: Option<String>,
    pub zooniverse_username: Option<String>,
    pub orcid: Option<String>,
    pub degree: Option<String>,
    pub ethnicity: Option<String>,
    pub income: Option<String>,
    pub scientist: Option<bool>,
    pub educator: Option<bool>,
    pub public: Option<bool>,
    pub protected: Option<bool>,
    pub accept_emails: Option<bool>,
    pub newsletter: Option<bool>,
    pub accept_messages: Option<bool>,
    pub track_contributions: Option<bool>,
    pub ai_opt_out: Option<bool>,
    pub extra_skills: Option<String>,
    pub extra_tools: Option<String>,
}

/// Privacy-respecting public view of a user. Returned by
/// `user.profile.public_get`.
///
/// The `visibility` field describes whether the viewer is allowed to
/// see profile content at all. When `Restricted`, the consumer should
/// render a placeholder using only `handle` and `image` (image may
/// also be omitted at the consumer's discretion).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicProfile {
    pub user_id: Ulid,
    pub handle: String,
    pub visibility: PublicProfileVisibility,
    pub first_name: String,
    pub last_name: String,
    pub image: Option<String>,
    pub bio: Option<String>,
    pub home_page: Option<String>,
    pub facebook_url: Option<String>,
    pub twitter_profile: Option<String>,
    pub inat_username: Option<String>,
    pub zooniverse_username: Option<String>,
    pub orcid: Option<String>,
    pub scientist: bool,
    pub educator: bool,
    pub ambassador: Option<String>,
    pub date_joined: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PublicProfileVisibility {
    Public,
    Restricted,
}

/// Application status for an ambassador application. Numeric values
/// match orrery's `APP_STATUS_CHOICES` for cross-system consistency.
/// (orrery's retired `Withdrawn = 9999` is intentionally omitted —
/// withdrawal is now handled by demoting the application to `Champion`.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i64)]
pub enum AmbassadorAppStatus {
    UnderReview = 0,
    Accepted = 1,
    Rejected = 2,
    Flagged = 3,
    Ineligible = 4,
    Champion = 5,
}

impl AmbassadorAppStatus {
    pub fn from_i64(v: i64) -> Self {
        match v {
            1 => AmbassadorAppStatus::Accepted,
            2 => AmbassadorAppStatus::Rejected,
            3 => AmbassadorAppStatus::Flagged,
            4 => AmbassadorAppStatus::Ineligible,
            5 => AmbassadorAppStatus::Champion,
            _ => AmbassadorAppStatus::UnderReview,
        }
    }

    pub fn as_i64(self) -> i64 {
        self as i64
    }

    /// Human-readable label (matches orrery's `APP_STATUS_CHOICES`).
    pub fn label(self) -> &'static str {
        match self {
            AmbassadorAppStatus::UnderReview => "Under review",
            AmbassadorAppStatus::Accepted => "Accepted",
            AmbassadorAppStatus::Rejected => "Rejected",
            AmbassadorAppStatus::Flagged => "Flagged",
            AmbassadorAppStatus::Ineligible => "Ineligible",
            AmbassadorAppStatus::Champion => "Champion",
        }
    }

    /// All variants, in menu order, for building selects in the admin grid.
    pub fn all() -> &'static [AmbassadorAppStatus] {
        &[
            AmbassadorAppStatus::UnderReview,
            AmbassadorAppStatus::Accepted,
            AmbassadorAppStatus::Rejected,
            AmbassadorAppStatus::Flagged,
            AmbassadorAppStatus::Ineligible,
            AmbassadorAppStatus::Champion,
        ]
    }
}

impl Default for AmbassadorAppStatus {
    fn default() -> Self {
        AmbassadorAppStatus::UnderReview
    }
}

/// Third-party background-screening status for an ambassador application.
/// Numeric values match orrery's `SCREENING_STATUS_CHOICES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i64)]
pub enum AmbassadorScreeningStatus {
    ToVerify = 0,
    Verified = 1,
    Flagged = 2,
    Failed = 3,
    International = 4,
}

impl AmbassadorScreeningStatus {
    pub fn from_i64(v: i64) -> Self {
        match v {
            1 => AmbassadorScreeningStatus::Verified,
            2 => AmbassadorScreeningStatus::Flagged,
            3 => AmbassadorScreeningStatus::Failed,
            4 => AmbassadorScreeningStatus::International,
            _ => AmbassadorScreeningStatus::ToVerify,
        }
    }

    pub fn as_i64(self) -> i64 {
        self as i64
    }

    /// Human-readable label (matches orrery's `SCREENING_STATUS_CHOICES`).
    pub fn label(self) -> &'static str {
        match self {
            AmbassadorScreeningStatus::ToVerify => "To verify",
            AmbassadorScreeningStatus::Verified => "Verified",
            AmbassadorScreeningStatus::Flagged => "Flagged",
            AmbassadorScreeningStatus::Failed => "Failed",
            AmbassadorScreeningStatus::International => "International",
        }
    }

    /// All variants, in menu order, for building selects in the admin grid.
    pub fn all() -> &'static [AmbassadorScreeningStatus] {
        &[
            AmbassadorScreeningStatus::ToVerify,
            AmbassadorScreeningStatus::Verified,
            AmbassadorScreeningStatus::Flagged,
            AmbassadorScreeningStatus::Failed,
            AmbassadorScreeningStatus::International,
        ]
    }
}

impl Default for AmbassadorScreeningStatus {
    fn default() -> Self {
        AmbassadorScreeningStatus::ToVerify
    }
}

/// Full state of a user's ambassador application. Returned by
/// `user.ambassador.get`. If the user has not yet started an
/// application the handler returns a default-valued record (status =
/// UnderReview, all text fields empty, `submitted` = None).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorApplication {
    pub user_id: Ulid,
    pub bio: String,
    pub city: String,
    pub state: String,
    pub contact_email: String,
    pub youtube_video_url: String,
    // Full applicant field set (backs the public apply-form prefill).
    pub phone: String,
    pub tee_size: String,
    pub self_description: String,
    pub handles: String,
    pub experience: String,
    pub motivation: String,
    pub related: String,
    pub vision: String,
    pub community_access: String,
    pub introduction: String,
    pub introduction_other: String,
    pub questions: String,
    pub signature_name: String,
    /// Comma-joined multi-selects (mirrors orrery's list fields).
    pub gender: String,
    pub ethnicity: String,
    pub activities: String,
    pub citscimonth: bool,
    pub background_check: bool,
    pub conduct: bool,
    pub birthdate: Option<String>,
    pub signature_date: Option<String>,
    pub draft: bool,
    pub app_status: AmbassadorAppStatus,
    pub submitted: Option<String>,
    pub updated_at: String,
}

/// One row of the staff ambassador-processing grid. Returned (as a `Vec`) by
/// `user.ambassador.list`. Carries the read-only applicant identity/contact
/// columns plus every staff-editable review field, mirroring orrery's
/// `AmbassadorApplicationAdmin` list. `in_directory` is the derived "passed"
/// state (accepted + screening verified) that puts the user in the public
/// directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorApplicationRow {
    pub user_id: Ulid,
    // Read-only applicant identity / contact
    pub name: String,
    pub handle: String,
    pub email: String,
    pub city: String,
    pub state: String,
    pub phone: String,
    pub birthdate: Option<String>,
    pub tee_size: String,
    pub gender: String,
    pub ethnicity: String,
    pub in_directory: bool,
    pub submitted: Option<String>,
    pub updated_at: String,
    // Staff-editable review fields
    pub marked: bool,
    pub archived: bool,
    pub hidden: bool,
    pub draft: bool,
    pub cohort: i64,
    pub app_status: AmbassadorAppStatus,
    pub screening_status: AmbassadorScreeningStatus,
    pub verified: Option<String>,
    pub rejection_sent: bool,
    pub welcome_sent: bool,
    pub orientation: Option<String>,
    pub kit_sent: Option<String>,
    pub survey: bool,
    pub handles: String,
    pub reviewers: String,
    pub notes: String,
}

/// A staff review edit of an ambassador application, sent via
/// `user.ambassador.review`. Setting these fields drives the native ambassador
/// state machine (mirrors orrery's `ambassador_application_pre_save`): granting
/// or revoking the Certified Ambassador badge, moving the applicant into or out
/// of the public directory, and demoting to Champion. Every field is present so
/// the grid can submit a full row; the handler diffs against the stored record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorReviewUpdate {
    pub user_id: Ulid,
    pub marked: bool,
    pub archived: bool,
    pub hidden: bool,
    pub draft: bool,
    pub cohort: i64,
    pub app_status: AmbassadorAppStatus,
    pub screening_status: AmbassadorScreeningStatus,
    pub verified: Option<String>,
    pub rejection_sent: bool,
    pub welcome_sent: bool,
    pub orientation: Option<String>,
    pub kit_sent: Option<String>,
    pub survey: bool,
    pub handles: String,
    pub reviewers: String,
    pub notes: String,
}

/// Query for the public ambassador directory (`user.ambassador.directory`).
/// `name` matches first/last name or bio; `near` matches city/state/ZIP.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AmbassadorDirectoryQuery {
    pub name: String,
    pub near: String,
    pub page: i64,
    pub order: AmbassadorDirectoryOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AmbassadorDirectoryOrder {
    #[default]
    Default,
    Name,
    Time,
}

/// One card in the public ambassador directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorDirectoryEntry {
    pub user_id: Ulid,
    pub name: String,
    pub city: String,
    pub state: String,
    pub image: Option<String>,
}

/// A page of the public ambassador directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorDirectoryResponse {
    pub page: i64,
    pub pages: i64,
    pub total: i64,
    pub entries: Vec<AmbassadorDirectoryEntry>,
}

/// A single public ambassador profile (`user.ambassador.profile`). `others` are
/// a few fellow ambassadors for cross-linking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorProfile {
    pub user_id: Ulid,
    pub name: String,
    pub image: Option<String>,
    pub city: String,
    pub state: String,
    pub bio: String,
    pub youtube_video_url: String,
    pub others: Vec<AmbassadorDirectoryEntry>,
}

/// A logged-in visitor's message to a directory ambassador
/// (`user.ambassador.message_send`). Queued for staff moderation before it is
/// emailed to the ambassador. Mirrors orrery's `ambassador_send` args.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorMessageForm {
    pub sender_id: Ulid,
    pub recipient_id: Ulid,
    pub first_name: String,
    pub last_name: String,
    pub subject: String,
    pub dates: String,
    pub body: String,
}

/// One queued contact message in the staff moderation grid
/// (`user.ambassador.message_list`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorMessageRow {
    pub id: Ulid,
    pub when: String,
    pub sender_id: Ulid,
    pub sender_email: String,
    pub recipient_id: Ulid,
    pub recipient_name: String,
    pub recipient_email: String,
    pub first_name: String,
    pub last_name: String,
    pub subject: String,
    pub dates: String,
    pub body: String,
    pub accepted: bool,
    pub sent: Option<String>,
}

/// Staff decision on a queued contact message
/// (`user.ambassador.message_review`). Accepting an un-sent message delivers it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorMessageReview {
    pub id: Ulid,
    pub accepted: bool,
}

/// Cohort label for display in the admin grid. Mirrors orrery's
/// `COHORT_CHOICES`: the special buckets 9998/9999 map to Hiatus/Withdrawn,
/// and every other id `n` displays as "Cohort {n + 1}".
pub fn ambassador_cohort_label(cohort: i64) -> String {
    match cohort {
        9998 => "Hiatus".to_string(),
        9999 => "Withdrawn".to_string(),
        n => format!("Cohort {}", n + 1),
    }
}

/// What kind of entity an `EntitySummary` describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntitySummaryKind {
    Project,
    Event,
}

/// Lightweight description of a project or event used by dashboard
/// panels that present lists of entities (bookmarks, owned, joined,
/// recently visited).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySummary {
    pub id: Ulid,
    pub kind: EntitySummaryKind,
    pub name: String,
    pub slug: String,
    pub image: Option<String>,
    pub status: String,
    pub created: String,
}

/// A user-saved point-of-interest. Returned by
/// `user.places.list_by_user`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPlace {
    pub id: Ulid,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub created: String,
}

/// Sent via `user.places.add`. The handler mints a new `id` and
/// inserts the row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPlaceAdd {
    pub user_id: Ulid,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// Sent via `user.places.delete`. The handler verifies that the place
/// belongs to `user_id` before deleting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPlaceDelete {
    pub user_id: Ulid,
    pub id: Ulid,
}

/// A user-issued API link token. Returned by `user.link_tokens.list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkToken {
    pub token: String,
    pub label: String,
    pub created: String,
    pub expires: Option<String>,
}

/// Sent via `user.link_tokens.add`. The handler mints a new token
/// string and returns the resulting `LinkToken`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkTokenAdd {
    pub user_id: Ulid,
    pub label: String,
    pub expires: Option<String>,
}

/// Sent via `user.link_tokens.delete`. The handler verifies that the
/// token belongs to `user_id` before deleting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkTokenDelete {
    pub user_id: Ulid,
    pub token: String,
}

/// Self-applicable update for an ambassador application. Sent via
/// `user.ambassador.update`. The handler upserts on `user_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorApplicationUpdate {
    pub user_id: Ulid,
    pub bio: String,
    pub city: String,
    pub state: String,
    pub contact_email: String,
    pub youtube_video_url: String,
}

/// Full ambassador application write, sent via `user.ambassador.save_draft`
/// (lax, `draft = true`) and `user.ambassador.submit` (final, `draft = false`,
/// stamps `submitted` and fires the application-submitted email + tag). Mirrors
/// orrery's `_write_ambassador_application_fields` + `_write_ambassador_user_fields`:
/// the applicant fields land on the application row, and the identity/address
/// fields (`first_name`..`birthdate`) land on the user record. Multi-selects
/// (`gender`/`ethnicity`/`activities`) are comma-joined by the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorApplicationForm {
    pub user_id: Ulid,
    // Application fields
    pub phone: String,
    pub city: String,
    pub state: String,
    pub tee_size: String,
    pub self_description: String,
    pub contact_email: String,
    pub handles: String,
    pub experience: String,
    pub motivation: String,
    pub related: String,
    pub vision: String,
    pub community_access: String,
    pub introduction: String,
    pub introduction_other: String,
    pub questions: String,
    pub signature_name: String,
    pub gender: String,
    pub ethnicity: String,
    pub activities: String,
    pub citscimonth: bool,
    pub background_check: bool,
    pub conduct: bool,
    pub birthdate: Option<String>,
    pub signature_date: Option<String>,
    // User-record fields
    pub first_name: String,
    pub last_name: String,
    pub address1: String,
    pub address2: String,
    pub zip: String,
}
