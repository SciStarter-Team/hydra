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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i64)]
pub enum AmbassadorAppStatus {
    UnderReview = 0,
    Accepted = 1,
    Rejected = 2,
    Flagged = 3,
    Ineligible = 4,
    Champion = 5,
    Withdrawn = 9999,
}

impl AmbassadorAppStatus {
    pub fn from_i64(v: i64) -> Self {
        match v {
            1 => AmbassadorAppStatus::Accepted,
            2 => AmbassadorAppStatus::Rejected,
            3 => AmbassadorAppStatus::Flagged,
            4 => AmbassadorAppStatus::Ineligible,
            5 => AmbassadorAppStatus::Champion,
            9999 => AmbassadorAppStatus::Withdrawn,
            _ => AmbassadorAppStatus::UnderReview,
        }
    }

    pub fn as_i64(self) -> i64 {
        self as i64
    }
}

impl Default for AmbassadorAppStatus {
    fn default() -> Self {
        AmbassadorAppStatus::UnderReview
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
    pub app_status: AmbassadorAppStatus,
    pub submitted: Option<String>,
    pub updated_at: String,
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
