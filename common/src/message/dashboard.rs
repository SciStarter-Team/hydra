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
/// see profile content at all. When `Restricted`, every identifying
/// field has already been emptied — `handle` reads as the private
/// placeholder and `image` is `None` — so a consumer can render the
/// whole struct without checking `visibility` first.
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
///
/// `Champion` and `Ambassador` are levers rather than notes: staff demote for a
/// hiatus and promote again on return, and orrery's state machine sets them
/// back when it moves of its own accord. They are each other's inverse, so
/// every variant here has to exist for the two systems to read the same row the
/// same way — an unlisted value decodes as `UnderReview`, which would read a
/// serving Ambassador as an unexamined applicant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i64)]
pub enum AmbassadorAppStatus {
    UnderReview = 0,
    Accepted = 1,
    Rejected = 2,
    Flagged = 3,
    Ineligible = 4,
    Champion = 5,
    Ambassador = 6,
}

impl AmbassadorAppStatus {
    pub fn from_i64(v: i64) -> Self {
        match v {
            1 => AmbassadorAppStatus::Accepted,
            2 => AmbassadorAppStatus::Rejected,
            3 => AmbassadorAppStatus::Flagged,
            4 => AmbassadorAppStatus::Ineligible,
            5 => AmbassadorAppStatus::Champion,
            6 => AmbassadorAppStatus::Ambassador,
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
            AmbassadorAppStatus::Ambassador => "Ambassador",
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
            AmbassadorAppStatus::Ambassador,
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

/// A filter that can require a boolean column to be true, require it to be
/// false, or not constrain it. Mirrors orrery's staff-browser `TriState`, and
/// renders as the same three-option select.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriState {
    #[default]
    Any,
    Yes,
    No,
}

impl TriState {
    pub fn as_bool(self) -> Option<bool> {
        match self {
            TriState::Any => None,
            TriState::Yes => Some(true),
            TriState::No => Some(false),
        }
    }

    pub fn is_any(self) -> bool {
        matches!(self, TriState::Any)
    }

    pub fn is_yes(self) -> bool {
        matches!(self, TriState::Yes)
    }

    pub fn is_no(self) -> bool {
        matches!(self, TriState::No)
    }
}

/// Whether a nullable date column is set at all, irrespective of its value.
/// Separate from [`TriState`] because it compiles to `IS NULL` / `IS NOT NULL`
/// rather than an equality test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatePresence {
    #[default]
    Any,
    Present,
    Absent,
}

impl DatePresence {
    pub fn is_any(self) -> bool {
        matches!(self, DatePresence::Any)
    }

    pub fn is_present(self) -> bool {
        matches!(self, DatePresence::Present)
    }

    pub fn is_absent(self) -> bool {
        matches!(self, DatePresence::Absent)
    }
}

/// Filters for the staff ambassador grid, mirroring orrery's
/// `/staff/ambassadors/applications`. Applied in SQL by user-service rather
/// than over the returned rows, so the grid pages rather than shipping the
/// whole table.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AmbassadorFilters {
    /// Restricts to a single applicant. Used to re-fetch one row after an
    /// inline save.
    pub user_id: Option<Ulid>,

    pub archived: TriState,
    pub draft: TriState,
    pub hidden: TriState,
    pub marked: TriState,
    pub welcome_sent: TriState,
    pub rejection_sent: TriState,
    pub survey: TriState,
    pub citscimonth: TriState,
    pub background_check: TriState,
    pub conduct: TriState,
    pub in_directory: TriState,

    pub cohort: Vec<i64>,
    pub app_status: Vec<i64>,
    pub screening_status: Vec<i64>,

    pub verified_presence: DatePresence,
    pub orientation_presence: DatePresence,
    pub kit_sent_presence: DatePresence,
    pub renewal_date_presence: DatePresence,

    pub submitted_from: String,
    pub submitted_to: String,
    pub verified_from: String,
    pub verified_to: String,
    pub renewal_date_from: String,
    pub renewal_date_to: String,

    /// Matched across name, handle and contact email at once.
    pub search: String,
    pub city: String,
    pub state: String,
    pub phone: String,
    pub tee_size: String,
    pub gender: String,
    pub ethnicity: String,
    pub handles: String,
    pub reviewers: String,
    pub notes: String,
}

/// Request for `user.ambassador.list`: which applications, and which page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AmbassadorListRequest {
    pub filters: AmbassadorFilters,
    /// 1-based. Zero or absent means the first page.
    pub page: i64,
    /// Zero means "every match, unpaged" — used by the CSV export.
    pub per_page: i64,
}

/// Response for `user.ambassador.list`: one page plus the total it came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorListResponse {
    pub rows: Vec<AmbassadorApplicationRow>,
    pub total: i64,
}

/// One month's arrivals at one milestone, for the Moore Foundation ledger
/// (`user.moore.buckets`).
///
/// Arrivals rather than running totals: accumulating in the caller keeps this
/// to one pass over each table, and a month with nobody in it simply has no
/// bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MooreBucket {
    /// Which milestone, as one of the ledger's metric tags.
    pub metric: String,
    /// The month, as `YYYY-MM`.
    pub month: String,
    /// How many people first reached the milestone that month.
    pub count: i64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MooreBucketsResponse {
    pub buckets: Vec<MooreBucket>,
}

/// One library in the network directory (`user.library.directory_record` to
/// add, `user.library.directory_list` to read).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LibraryDirectoryEntry {
    pub name: String,
    pub address: String,
    pub city: String,
    pub state: String,
    pub zipcode: String,
    pub org_type: String,
    pub url: String,
    /// The library's location as a GeoJSON `Point`, or empty when the address
    /// could not be resolved. A directory row without one is still a library;
    /// it just cannot be mapped.
    pub point: String,
    /// Empty on the way in; stamped by the service.
    pub recorded: String,
}

/// The result of adding one directory entry.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LibraryDirectoryRecordResponse {
    /// False when a library of that name was already in the directory, so the
    /// entry was left alone. The import reports these as "already known"
    /// rather than as failures.
    pub added: bool,
}

/// An application to host an Acts of Science Connected event
/// (`user.acts_connected.record` to store, `user.acts_connected.list` to read
/// them all back).
///
/// One struct serves both directions: the form fills in everything but
/// `signup_id` and `recorded`, which the service stamps, and the staff export
/// reads the whole thing back. A separate "stored" type would be the same
/// fields twice.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ActsConnectedSignup {
    /// Empty on the way in; filled in by the service.
    pub signup_id: String,
    /// Empty on the way in; stamped by the service.
    pub recorded: String,

    pub library_name: String,
    pub address1: String,
    pub address2: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub country: String,

    pub contact_first: String,
    pub contact_last: String,
    pub contact_email: String,
    pub contact_phone: String,

    pub service_area: String,
    pub seating_capacity: String,
    pub tech_support: String,

    /// Which of the offered date-and-time slots the applicant can host, as
    /// free text — the form lets people qualify an answer, and staff read that
    /// text when scheduling.
    pub event1aft: String,
    pub event1eve: String,
    pub event2aft: String,
    pub event2eve: String,
    pub event3aft: String,
    pub event3mid: String,
    pub event3eve: String,
    pub event4aft: String,
    pub event4eve: String,
    pub event5aft: String,
    pub event5eve: String,
    pub eventpromote: String,

    pub additional_programming: String,
    pub why_interested: String,
    pub requirements: String,
    pub wired_ethernet: String,
    pub available_devices: String,
    pub comments: String,
    pub request_grant: String,
}

/// A stage of the Ambassador funnel.
///
/// Derived from the milestones a person has reached rather than stored: see
/// [`AmbassadorStage::derive`]. There is no `Initial` variant, because "has
/// reached no stage" is the absence of a stage — [`AmbassadorStage::derive`]
/// returns `None` for it, and no funnel export has a row to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmbassadorStage {
    /// Champion training finished.
    Tutorial,
    Application,
    Verified,
    /// An Ambassador: in the public directory.
    Passed,
    International,
    Rejected,
    Failed,
}

/// The milestones an Ambassador funnel stage is derived from.
///
/// Each field is a fact recorded somewhere in its own right — a timestamp on
/// the progress row, a date or a verdict on the application, the directory
/// date on the user — so that the stage stays a view of them and cannot
/// contradict them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmbassadorMilestones {
    /// `ambassador_progress.tutorial_completed_at` is set.
    pub tutorial_completed: bool,
    /// A non-draft application with a `submitted` date exists.
    pub application_submitted: bool,
    /// The application carries a `verified` date.
    pub verified: bool,
    /// `users.ambassador` is set: the person is in the public directory.
    pub in_directory: bool,
    pub app_status: AmbassadorAppStatus,
    pub screening_status: AmbassadorScreeningStatus,
}

impl AmbassadorStage {
    /// The stage these milestones add up to, or `None` for somebody who has
    /// not started.
    ///
    /// The precedence is orrery's `ambassador_application_pre_save`: the two
    /// levers first — demotion to Champion, then promotion to Ambassador — then
    /// the ways the route can close, then how far along the open route the
    /// person got. Staff's verdict on the application therefore wins over the
    /// milestones it supersedes — demoting an Ambassador makes them a Champion
    /// again on the strength of the verdict alone, without disturbing the dates
    /// that record how they got there.
    pub fn derive(milestones: &AmbassadorMilestones) -> Option<AmbassadorStage> {
        if milestones.app_status == AmbassadorAppStatus::Champion {
            // The demotion lever, and it reaches back to Champion from any
            // later stage. Somebody demoted before ever finishing the training
            // is nowhere in the funnel rather than a Champion.
            return milestones
                .tutorial_completed
                .then_some(AmbassadorStage::Tutorial);
        }

        if milestones.app_status == AmbassadorAppStatus::Ambassador {
            // The promotion lever, and orrery checks it before the ways the
            // route can close: staff promoting somebody is the most recent word
            // on them, and orrery does not make the promotion wait on screening.
            return Some(AmbassadorStage::Passed);
        }

        if let Some(closed) =
            AmbassadorStage::closed_by(milestones.app_status, milestones.screening_status)
        {
            return Some(closed);
        }

        if milestones.in_directory {
            Some(AmbassadorStage::Passed)
        } else if milestones.verified {
            Some(AmbassadorStage::Verified)
        } else if milestones.application_submitted {
            Some(AmbassadorStage::Application)
        } else if milestones.tutorial_completed {
            Some(AmbassadorStage::Tutorial)
        } else {
            None
        }
    }

    /// The stage at which a verdict closes the route past Champion, if it
    /// closes it at all.
    ///
    /// Separate from [`AmbassadorStage::derive`], which calls it, because the
    /// caller recording that the route closed has to agree with the caller
    /// reporting where somebody stands about which verdicts close it.
    pub fn closed_by(
        app_status: AmbassadorAppStatus,
        screening_status: AmbassadorScreeningStatus,
    ) -> Option<AmbassadorStage> {
        if app_status == AmbassadorAppStatus::Rejected {
            return Some(AmbassadorStage::Rejected);
        }

        match screening_status {
            AmbassadorScreeningStatus::International => Some(AmbassadorStage::International),
            AmbassadorScreeningStatus::Failed => Some(AmbassadorStage::Failed),
            _ => None,
        }
    }

    pub const ALL: &'static [AmbassadorStage] = &[
        AmbassadorStage::Tutorial,
        AmbassadorStage::Application,
        AmbassadorStage::Verified,
        AmbassadorStage::Passed,
        AmbassadorStage::International,
        AmbassadorStage::Rejected,
        AmbassadorStage::Failed,
    ];

    /// What staff call this stage.
    pub fn label(self) -> &'static str {
        match self {
            AmbassadorStage::Tutorial => "Champion training complete",
            AmbassadorStage::Application => "Application submitted",
            AmbassadorStage::Verified => "Application verified",
            AmbassadorStage::Passed => "Ambassador",
            AmbassadorStage::International => "International",
            AmbassadorStage::Rejected => "Rejected",
            AmbassadorStage::Failed => "Screening failed",
        }
    }
}

/// Which stages of the Ambassador funnel to export (`user.ambassador.stages`).
///
/// The five staff funnel exports — one per stage, plus the Champions list that
/// spans three — are the same question asked of different stages, so they share
/// one request rather than one endpoint each.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AmbassadorStagesRequest {
    /// The stages to include. An empty list matches nothing, so a caller
    /// cannot accidentally export everyone.
    pub stages: Vec<AmbassadorStage>,
}

/// One person at one stage of the Ambassador funnel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorStageRow {
    pub first_name: String,
    pub last_name: String,
    pub handle: String,
    pub email: String,
    pub stage: AmbassadorStage,
    /// The date the person reached that stage, as `YYYY-MM-DD`, or empty when
    /// the milestone it is derived from carries no date.
    pub since: String,
}

/// One row of the staff CSV export (`user.ambassador.export`). Wider than the
/// grid row: it carries the applicant's long-form answers too, matching what
/// orrery's export produces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorExportRow {
    pub user_ulid: String,
    pub username: String,
    pub first_name: String,
    pub last_name: String,
    pub handle: String,
    pub email: String,
    pub in_directory: bool,
    pub submitted: Option<String>,
    pub updated_at: String,
    pub draft: bool,
    pub hidden: bool,
    pub marked: bool,
    pub archived: bool,
    pub cohort: i64,
    pub app_status: String,
    pub screening_status: String,
    pub verified: Option<String>,
    pub orientation: Option<String>,
    pub kit_sent: Option<String>,
    pub renewal_date: Option<String>,
    pub birthdate: Option<String>,
    pub welcome_sent: bool,
    pub rejection_sent: bool,
    pub survey: bool,
    pub citscimonth: bool,
    pub background_check: bool,
    pub conduct: bool,
    pub phone: String,
    pub city: String,
    pub state: String,
    pub tee_size: String,
    pub gender: String,
    pub ethnicity: String,
    pub self_description: String,
    pub handles: String,
    pub introduction: String,
    pub introduction_other: String,
    pub experience: String,
    pub motivation: String,
    pub related: String,
    pub vision: String,
    pub activities: String,
    pub community_access: String,
    pub signature_name: String,
    pub signature_date: Option<String>,
    pub questions: String,
    pub bio: String,
    pub notes: String,
    pub reviewers: String,
    pub youtube_video_url: String,
}

/// Query for the public ambassador directory (`user.ambassador.directory`).
/// `name` matches a first or last name, the whole name either way round, or
/// the bio; `near` is "City, ST" for an exact place, an all-capitals state
/// code, or otherwise any part of a city, state or ZIP.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AmbassadorDirectoryQuery {
    pub name: String,
    pub near: String,
    pub page: i64,
    pub order: AmbassadorDirectoryOrder,
}

/// How the public directory is sorted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AmbassadorDirectoryOrder {
    /// A shuffle that holds still: each application draws a random position
    /// once, so nobody is permanently at the top of an alphabetical list and
    /// paging does not repeat or skip anyone.
    #[default]
    Random,
    Name,
    /// Most recently certified first.
    Time,
}

impl AmbassadorDirectoryOrder {
    /// The `order` query-string value, or `None` for the default.
    pub fn as_param(self) -> Option<&'static str> {
        match self {
            AmbassadorDirectoryOrder::Random => None,
            AmbassadorDirectoryOrder::Name => Some("name"),
            AmbassadorDirectoryOrder::Time => Some("time"),
        }
    }

    /// Read an `order` query-string value; anything unrecognised is the default.
    pub fn from_param(param: &str) -> Self {
        match param {
            "name" => AmbassadorDirectoryOrder::Name,
            "time" => AmbassadorDirectoryOrder::Time,
            _ => AmbassadorDirectoryOrder::Random,
        }
    }
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

/// Request for one ambassador profile (`user.ambassador.profile`).
///
/// The viewer matters because a profile that is not listed publicly (not yet
/// promoted, demoted, or hidden) can still be previewed by its owner and by
/// staff, the way orrery lets them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorProfileQuery {
    pub user_id: Ulid,
    pub viewer: Option<Ulid>,
    pub viewer_is_staff: bool,
}

/// A single ambassador profile (`user.ambassador.profile`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorProfile {
    pub user_id: Ulid,
    pub name: String,
    pub first_name: String,
    pub image: Option<String>,
    pub city: String,
    pub state: String,
    /// Markdown written by the ambassador.
    pub bio: String,
    pub youtube_video_url: String,
    /// Whether the public directory lists this person. `false` only reaches
    /// the owner or staff, previewing.
    pub listed: bool,
    /// Events staff (or orrery, for an event's owner) linked to this person as
    /// an ambassador. The frontend owns the event data, so it narrows these to
    /// published ambassador events and adds the events the person manages.
    pub connected_event_ids: Vec<Ulid>,
    /// A few other listed ambassadors in the same state.
    pub nearby: Vec<AmbassadorDirectoryEntry>,
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
    /// Event start time (RFC 3339), if the entity is an event with a
    /// scheduled start. Always `None` for projects.
    #[serde(default)]
    pub begin: Option<String>,
    /// Event end time (RFC 3339), if scheduled. Always `None` for projects.
    #[serde(default)]
    pub end: Option<String>,
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

/// A Take Action project logged by a Girl Scout troop at `/girlscouts-action`,
/// the last step of the Think Like a Citizen Scientist Journey. Sent via
/// `user.girlscouts.take_action_submit`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakeActionSubmission {
    /// The signed-in submitter. Orrery accepted these anonymously and stored
    /// no timestamp, which left staff with write-ups they could not follow up
    /// on; the submitter is required here and the handler stamps the time.
    pub user_id: Ulid,
    pub troop: String,
    pub problem: String,
    pub solution: String,
    pub sustainable: String,
    /// URL of the troop photo, empty when none was attached.
    pub photo: String,
}

/// One Take Action submission as staff read it, with the submitter resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakeActionRow {
    pub submission_id: Ulid,
    pub user_id: Ulid,
    pub submitted: String,
    // Read-only submitter identity, joined from the user record
    pub name: String,
    pub handle: String,
    pub email: String,
    pub troop: String,
    pub problem: String,
    pub solution: String,
    pub sustainable: String,
    pub photo: String,
}

/// One page of the Take Action queue, requested via
/// `user.girlscouts.take_action_list`. Submissions accumulate indefinitely, so
/// the queue is paged rather than returned whole.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakeActionListQuery {
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakeActionList {
    pub rows: Vec<TakeActionRow>,
    /// Total across every page, for the pager.
    pub total: i64,
}

/// Filters for the staff Library Network browser, mirroring orrery's
/// `/staff/libraries`. Applied in SQL by user-service so the grid pages rather
/// than shipping every signup.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibrarySignupFilters {
    pub add_to_map: TriState,
    pub have_account: Vec<i64>,

    pub recorded_from: String,
    pub recorded_to: String,

    /// Matched across organization name, contact name and email at once.
    pub search: String,
    pub organization_name: String,
    pub organization_type: String,
    pub primary_job_type: String,
    pub organization_city: String,
    pub organization_state: String,
    pub organization_zip_code: String,
    pub organization_country: String,
    pub organization_web_page: String,
    pub email: String,
    pub job_title: String,
}

/// Request for `user.library.list`: which signups, and which page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibrarySignupListRequest {
    pub filters: LibrarySignupFilters,
    /// 1-based. Zero or absent means the first page.
    pub page: i64,
    /// Zero means "every match, unpaged" — used by the CSV export.
    pub per_page: i64,
}

/// One Library Network signup, as the staff grid and CSV export show it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySignupRow {
    pub signup_id: String,
    /// The row's id in orrery, while orrery still owns the sign-up form.
    /// Absent for signups created here.
    pub orrery_id: Option<i64>,
    pub recorded: String,
    pub user_ulid: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub job_title: String,
    pub have_account: String,
    pub organization_name: String,
    pub organization_type: String,
    pub primary_job_type: String,
    pub organization_street_address: String,
    pub organization_city: String,
    pub organization_state: String,
    pub organization_zip_code: String,
    pub organization_country: String,
    pub organization_web_page: String,
    pub organization_social_handles: String,
    pub add_to_map: bool,
}

/// Response for `user.library.list`: one page plus the total it came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySignupListResponse {
    pub rows: Vec<LibrarySignupRow>,
    pub total: i64,
}

/// One row of the "Prepare Map" export (`user.library.map`): the columns a
/// mapping tool wants, one per distinct organization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryMapRow {
    pub organization_name: String,
    pub organization_street_address: String,
    pub organization_city: String,
    pub organization_state: String,
    pub organization_zip_code: String,
    pub organization_country: String,
    pub organization_web_page: String,
}

/// A staff edit of a Library Network signup (`user.library.save`), or a new
/// signup from the public form (`user.library.create`). Every field the form
/// collects is present; identity and provenance columns are not editable.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibrarySignupUpdate {
    /// Empty when creating.
    pub signup_id: String,
    /// Only set when creating, from the session.
    pub user_ulid: Option<String>,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub job_title: String,
    pub have_account: i64,
    pub organization_name: String,
    pub organization_type: String,
    pub primary_job_type: String,
    pub organization_street_address: String,
    pub organization_city: String,
    pub organization_state: String,
    pub organization_zip_code: String,
    pub organization_country: String,
    pub organization_web_page: String,
    pub organization_social_handles: String,
    pub add_to_map: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Somebody who has reached nothing at all.
    fn nobody() -> AmbassadorMilestones {
        AmbassadorMilestones {
            tutorial_completed: false,
            application_submitted: false,
            verified: false,
            in_directory: false,
            app_status: AmbassadorAppStatus::UnderReview,
            screening_status: AmbassadorScreeningStatus::ToVerify,
        }
    }

    /// An Ambassador: every milestone along the open route reached.
    fn ambassador() -> AmbassadorMilestones {
        AmbassadorMilestones {
            tutorial_completed: true,
            application_submitted: true,
            verified: true,
            in_directory: true,
            app_status: AmbassadorAppStatus::Accepted,
            screening_status: AmbassadorScreeningStatus::Verified,
        }
    }

    #[test]
    fn no_milestone_is_no_stage() {
        assert_eq!(AmbassadorStage::derive(&nobody()), None);
    }

    /// The open route, walked one milestone at a time.
    #[test]
    fn each_milestone_advances_the_stage() {
        let mut person = nobody();
        assert_eq!(AmbassadorStage::derive(&person), None);

        person.tutorial_completed = true;
        assert_eq!(
            AmbassadorStage::derive(&person),
            Some(AmbassadorStage::Tutorial)
        );

        person.application_submitted = true;
        assert_eq!(
            AmbassadorStage::derive(&person),
            Some(AmbassadorStage::Application)
        );

        person.verified = true;
        assert_eq!(
            AmbassadorStage::derive(&person),
            Some(AmbassadorStage::Verified)
        );

        person.in_directory = true;
        assert_eq!(
            AmbassadorStage::derive(&person),
            Some(AmbassadorStage::Passed)
        );
    }

    /// Demotion is a verdict, not an erasure.
    ///
    /// Staff demote an Ambassador for a hiatus and promote them again on
    /// return, so the demotion has to read as Champion while the milestones
    /// that got them there stay exactly as they were.
    #[test]
    fn demotion_returns_an_ambassador_to_champion() {
        let mut person = ambassador();
        person.app_status = AmbassadorAppStatus::Champion;
        // The caller clears the directory date in the same edit; the verdict
        // has to hold even before it does.
        assert_eq!(
            AmbassadorStage::derive(&person),
            Some(AmbassadorStage::Tutorial)
        );

        person.app_status = AmbassadorAppStatus::Accepted;
        assert_eq!(
            AmbassadorStage::derive(&person),
            Some(AmbassadorStage::Passed),
            "promoting again should restore the stage the milestones support"
        );
    }

    /// A demotion cannot invent a Champion out of somebody who never trained.
    #[test]
    fn demoting_an_untrained_person_is_no_stage() {
        let mut person = nobody();
        person.app_status = AmbassadorAppStatus::Champion;
        assert_eq!(AmbassadorStage::derive(&person), None);
    }

    /// Each way the route can close outranks how far along it the person got.
    #[test]
    fn a_closed_route_outranks_progress_along_it() {
        for (person, expected) in [
            (
                AmbassadorMilestones {
                    app_status: AmbassadorAppStatus::Rejected,
                    ..ambassador()
                },
                AmbassadorStage::Rejected,
            ),
            (
                AmbassadorMilestones {
                    screening_status: AmbassadorScreeningStatus::International,
                    ..ambassador()
                },
                AmbassadorStage::International,
            ),
            (
                AmbassadorMilestones {
                    screening_status: AmbassadorScreeningStatus::Failed,
                    ..ambassador()
                },
                AmbassadorStage::Failed,
            ),
        ] {
            assert_eq!(AmbassadorStage::derive(&person), Some(expected));
        }
    }

    /// The promotion lever reaches past a closed route, and past screening.
    ///
    /// orrery promotes on the verdict alone — its `promote_to_ambassador` says
    /// in as many words that screening is a prerequisite staff are trusted to
    /// have checked, not one the code enforces — so a promoted Ambassador whose
    /// screening row still says otherwise is an Ambassador.
    #[test]
    fn promotion_outranks_a_closed_route() {
        let person = AmbassadorMilestones {
            app_status: AmbassadorAppStatus::Ambassador,
            screening_status: AmbassadorScreeningStatus::Failed,
            in_directory: false,
            ..ambassador()
        };

        assert_eq!(
            AmbassadorStage::derive(&person),
            Some(AmbassadorStage::Passed)
        );
    }

    /// ...but demotion outranks promotion, because it is its inverse and comes
    /// first in orrery's precedence.
    #[test]
    fn demotion_outranks_promotion() {
        let person = AmbassadorMilestones {
            app_status: AmbassadorAppStatus::Champion,
            ..ambassador()
        };

        assert_eq!(
            AmbassadorStage::derive(&person),
            Some(AmbassadorStage::Tutorial)
        );
    }

    /// Every value orrery can store has to decode to its own variant.
    ///
    /// An unlisted one decodes as `UnderReview`, which would read a serving
    /// Ambassador as an unexamined applicant — and did, for the 120 accounts
    /// carrying orrery's `AMBASSADOR` verdict before it had a variant here.
    #[test]
    fn every_app_status_orrery_writes_round_trips() {
        for status in AmbassadorAppStatus::all() {
            assert_eq!(
                AmbassadorAppStatus::from_i64(status.as_i64()),
                *status,
                "{status:?} does not survive the database"
            );
        }
    }

    /// Champion outranks a closed route, matching orrery's precedence: a
    /// demotion is staff's most recent word on the application.
    #[test]
    fn demotion_outranks_a_closed_screening() {
        let person = AmbassadorMilestones {
            app_status: AmbassadorAppStatus::Champion,
            screening_status: AmbassadorScreeningStatus::International,
            ..ambassador()
        };

        assert_eq!(
            AmbassadorStage::derive(&person),
            Some(AmbassadorStage::Tutorial)
        );
    }

    #[test]
    fn every_stage_is_reachable_and_labelled() {
        for stage in AmbassadorStage::ALL {
            assert!(!stage.label().is_empty());
        }
    }
}
