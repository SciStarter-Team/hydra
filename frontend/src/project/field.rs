use std::collections::HashSet;

use anyhow::Error;
use common::{insensitivestring::InsensitiveString, shared::db, timestring::TimeString};
use ulid::Ulid;

use crate::{contact::Contact, peering::LoadStorePeered};

use super::{ProjectParticipationDegree, ProjectPromotionTime, ProjectStatus};

// Note: The `tags_text` database column is NOT included in ProjectFieldType
// as it's a computed field for FTS search optimization, not a real project field.
// Tags are managed separately via the project_tags table and dedicated handlers.

#[derive(Debug, Clone, Copy)]
pub enum ProjectFieldType {
    ProjectId,
    LegacyV1ProjectId,
    LegacyV2ProjectId,
    ForeignId,
    Publish,
    Duplicate,
    Authoritative,
    Flagged,
    IsTest,
    Status,
    PromotionTime,
    PromotionTarget,
    AdminNotes,
    Created,
    WildHope,
    Exchange,
    Origin,
    Suborigin,
    Guid,
    OpengraphDescription,
    IsAffiliate,
    IsInaturalist,
    InatProjectId,
    Welcomed,
    PageCreatorId,
    PageEditorIds,
    Extra,
    ProjectLastEditor,
    ProjectLastEdited,
    ScistarterLastEditor,
    ScistarterLastEdited,
    Draft,
    Name,
    Tags,
    ParticipationDegree,
    ParticipantInstructions,
    ParticipantInstructionsEmail,
    ParticipantFiles,
    UsFederallyFunded,
    Presenter,
    Goal,
    Task,
    Description,
    Image,
    ImageCredit,
    ConnectOpaqueUrl,
    ConnectOpaqueUrl1,
    ConnectOpaqueUrl2,
    Contacts,
    Url,
    VideoUrl,
    BlogUrl,
    TwitterName,
    FacebookPage,
    InstagramPage,
    Topics,
    FieldsOfScience,
    Practices,
    PracticesHow,
    SearchTerms,
    Difficulty,
    Diy,
    Audience,
    SignupUrl,
    PrefixHowToLoggedIn,
    PrefixHowToLoggedOut,
    HowToJoin,
    HowtoVideo,
    OutcomesLabel,
    OutcomesLink,
    TimeRequired,
    TrainingMaterials,
    ClassroomMaterials,
    ConsentStatement,
    ConsentStatementDate,
    ParticipationTools,
    ParticipationPrompt,
    ParticipationConfirmationMessage,
    ParticipationNotificationEmail,
    ParticipationUsage,
    PartOf,
    AssociatedProjects,
    TotalExpense,
    Materials,
    Gear,
    MobileAppUrlAndroid,
    MobileAppUrlIos,
    MobileAppUrl,
    LocationType,
    EntirelyOnline,
    OnDemand,
    InPerson,
    Indoors,
    Outdoors,
    Regions,
    RegionsDescription,
    Point,
    Address,
    City,
    State,
    Zip,
    Country,
    Other,
    LocationName,
    LocationLanduse,
    LocationDescription,
    GeographicScope,
    UnitedNationsRegion,
    Begin,
    End,
    ProjectTiming,
    ParticipationSchedule,
    TimePerParticipation,
    YearStarted,
    YearCompleted,
    MonthCompleted,
    ParticipationTasks,
    Activities,
    SpecialSkills,
    DataResults,
    DataAggregation,
    DataConsent,
    DataVisualization,
    DataAccess,
    DataQuality,
    MetadataPublicationUrl,
    DataPublicationUrl,
    DataLicense,
    IntendedOutcomes,
    OutcomeOther,
    UniversityAffiliation,
    Sponsors,
    Stakeholders,
    CircuitId,
    SustainableDevelopmentGoals,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ProjectField {
    pub project_id: Ulid,
    pub field: ProjectFieldValue,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum ProjectFieldValue {
    ProjectId(Ulid),
    LegacyV1ProjectId(Option<i64>),
    LegacyV2ProjectId(Option<i64>),
    ForeignId(Option<String>),
    Publish(bool),
    Duplicate(bool),
    Authoritative(bool),
    Flagged(bool),
    IsTest(bool),
    Status(ProjectStatus),
    PromotionTime(ProjectPromotionTime),
    PromotionTarget(Option<TimeString>),
    AdminNotes(String),
    Created(TimeString),
    WildHope(bool),
    Exchange(Option<String>),
    Origin(String),
    Suborigin(String),
    Guid(String),
    OpengraphDescription(String),
    IsAffiliate(bool),
    IsInaturalist(bool),
    InatProjectId(Option<i32>),
    Welcomed(bool),
    PageCreatorId(Ulid),
    PageManagerIds(Box<[Ulid]>),
    Extra(String),
    ProjectLastEditor(Option<Ulid>),
    ProjectLastEdited(Option<TimeString>),
    ScistarterLastEditor(Option<Ulid>),
    ScistarterLastEdited(Option<TimeString>),
    Draft(bool),
    Name(String),
    Tags(HashSet<InsensitiveString>),
    ParticipationDegree(ProjectParticipationDegree),
    ParticipantInstructions(String),
    ParticipantInstructionsEmail(String),
    ParticipantFiles(Box<[String]>),
    UsFederallyFunded(bool),
    Presenter(String),
    Goal(String),
    Task(String),
    Description(String),
    Image(String),
    ImageCredit(String),
    ConnectOpaqueUrl(String),
    ConnectOpaqueUrl1(String),
    ConnectOpaqueUrl2(String),
    Contacts(Box<[Contact]>),
    Url(String),
    VideoUrl(String),
    BlogUrl(String),
    TwitterName(String),
    FacebookPage(String),
    InstagramPage(String),
    Topics(Box<[String]>),
    FieldsOfScience(Box<[String]>),
    Practices(Box<[String]>),
    PracticesHow(String),
    SearchTerms(String),
    Difficulty(String),
    Diy(bool),
    Audience(Box<[String]>),
    SignupUrl(String),
    PrefixHowToLoggedIn(String),
    PrefixHowToLoggedOut(String),
    HowToJoin(String),
    HowtoVideo(String),
    OutcomesLabel(String),
    OutcomesLink(String),
    TimeRequired(String),
    TrainingMaterials(String),
    ClassroomMaterials(String),
    ConsentStatement(String),
    ConsentStatementDate(Option<TimeString>),
    ParticipationTools(bool),
    ParticipationPrompt(String),
    ParticipationConfirmationMessage(String),
    ParticipationNotificationEmail(String),
    ParticipationUsage(String),
    PartOf(Option<Ulid>),
    AssociatedProjects(Box<[Ulid]>),
    TotalExpense(Option<f32>),
    Materials(String),
    Gear(String),
    MobileAppUrlAndroid(String),
    MobileAppUrlIos(String),
    MobileAppUrl(String),
    LocationType(String),
    EntirelyOnline(bool),
    OnDemand(bool),
    InPerson(bool),
    Indoors(bool),
    Outdoors(bool),
    Regions(Option<String>),
    RegionsDescription(String),
    Point(Option<String>),
    Address(String),
    City(String),
    State(String),
    Zip(String),
    Country(String),
    Other(String),
    LocationName(String),
    LocationLanduse(String),
    LocationDescription(String),
    GeographicScope(Box<[String]>),
    UnitedNationsRegion(String),
    Begin(Option<TimeString>),
    End(Option<TimeString>),
    ProjectTiming(String),
    ParticipationSchedule(String),
    TimePerParticipation(String),
    YearStarted(Option<i32>),
    YearCompleted(Option<i32>),
    MonthCompleted(Option<i32>),
    ParticipationTasks(Box<[String]>),
    Activities(Box<[String]>),
    SpecialSkills(String),
    DataResults(String),
    DataAggregation(Box<[String]>),
    DataConsent(Box<[String]>),
    DataVisualization(bool),
    DataAccess(Box<[String]>),
    DataQuality(bool),
    MetadataPublicationUrl(String),
    DataPublicationUrl(String),
    DataLicense(String),
    IntendedOutcomes(Box<[String]>),
    OutcomeOther(String),
    UniversityAffiliation(String),
    Sponsors(Box<[String]>),
    Stakeholders(Box<[String]>),
    CircuitId(String),
    SustainableDevelopmentGoals(String),
}

impl ProjectField {
    /// Create a new ProjectField with the given project_id and field value
    pub fn new(project_id: Ulid, field: ProjectFieldValue) -> Self {
        Self { project_id, field }
    }

    #[tracing::instrument]
    pub fn field_type(&self) -> ProjectFieldType {
        match &self.field {
            ProjectFieldValue::ProjectId(_) => ProjectFieldType::ProjectId,
            ProjectFieldValue::LegacyV1ProjectId(_) => ProjectFieldType::LegacyV1ProjectId,
            ProjectFieldValue::LegacyV2ProjectId(_) => ProjectFieldType::LegacyV2ProjectId,
            ProjectFieldValue::ForeignId(_) => ProjectFieldType::ForeignId,
            ProjectFieldValue::Publish(_) => ProjectFieldType::Publish,
            ProjectFieldValue::Duplicate(_) => ProjectFieldType::Duplicate,
            ProjectFieldValue::Authoritative(_) => ProjectFieldType::Authoritative,
            ProjectFieldValue::Flagged(_) => ProjectFieldType::Flagged,
            ProjectFieldValue::IsTest(_) => ProjectFieldType::IsTest,
            ProjectFieldValue::Status(_) => ProjectFieldType::Status,
            ProjectFieldValue::PromotionTime(_) => ProjectFieldType::PromotionTime,
            ProjectFieldValue::PromotionTarget(_) => ProjectFieldType::PromotionTarget,
            ProjectFieldValue::AdminNotes(_) => ProjectFieldType::AdminNotes,
            ProjectFieldValue::Created(_) => ProjectFieldType::Created,
            ProjectFieldValue::WildHope(_) => ProjectFieldType::WildHope,
            ProjectFieldValue::Exchange(_) => ProjectFieldType::Exchange,
            ProjectFieldValue::Origin(_) => ProjectFieldType::Origin,
            ProjectFieldValue::Suborigin(_) => ProjectFieldType::Suborigin,
            ProjectFieldValue::Guid(_) => ProjectFieldType::Guid,
            ProjectFieldValue::OpengraphDescription(_) => ProjectFieldType::OpengraphDescription,
            ProjectFieldValue::IsAffiliate(_) => ProjectFieldType::IsAffiliate,
            ProjectFieldValue::IsInaturalist(_) => ProjectFieldType::IsInaturalist,
            ProjectFieldValue::InatProjectId(_) => ProjectFieldType::InatProjectId,
            ProjectFieldValue::Welcomed(_) => ProjectFieldType::Welcomed,
            ProjectFieldValue::PageCreatorId(_) => ProjectFieldType::PageCreatorId,
            ProjectFieldValue::PageManagerIds(_) => ProjectFieldType::PageEditorIds,
            ProjectFieldValue::Extra(_) => ProjectFieldType::Extra,
            ProjectFieldValue::ProjectLastEditor(_) => ProjectFieldType::ProjectLastEditor,
            ProjectFieldValue::ProjectLastEdited(_) => ProjectFieldType::ProjectLastEdited,
            ProjectFieldValue::ScistarterLastEditor(_) => ProjectFieldType::ScistarterLastEditor,
            ProjectFieldValue::ScistarterLastEdited(_) => ProjectFieldType::ScistarterLastEdited,
            ProjectFieldValue::Draft(_) => ProjectFieldType::Draft,
            ProjectFieldValue::Name(_) => ProjectFieldType::Name,
            ProjectFieldValue::Tags(_) => ProjectFieldType::Tags,
            ProjectFieldValue::ParticipationDegree(_) => ProjectFieldType::ParticipationDegree,
            ProjectFieldValue::ParticipantInstructions(_) => ProjectFieldType::ParticipantInstructions,
            ProjectFieldValue::ParticipantInstructionsEmail(_) => {
                ProjectFieldType::ParticipantInstructionsEmail
            }
            ProjectFieldValue::ParticipantFiles(_) => ProjectFieldType::ParticipantFiles,
            ProjectFieldValue::UsFederallyFunded(_) => ProjectFieldType::UsFederallyFunded,
            ProjectFieldValue::Presenter(_) => ProjectFieldType::Presenter,
            ProjectFieldValue::Goal(_) => ProjectFieldType::Goal,
            ProjectFieldValue::Task(_) => ProjectFieldType::Task,
            ProjectFieldValue::Description(_) => ProjectFieldType::Description,
            ProjectFieldValue::Image(_) => ProjectFieldType::Image,
            ProjectFieldValue::ImageCredit(_) => ProjectFieldType::ImageCredit,
            ProjectFieldValue::ConnectOpaqueUrl(_) => ProjectFieldType::ConnectOpaqueUrl,
            ProjectFieldValue::ConnectOpaqueUrl1(_) => ProjectFieldType::ConnectOpaqueUrl1,
            ProjectFieldValue::ConnectOpaqueUrl2(_) => ProjectFieldType::ConnectOpaqueUrl2,
            ProjectFieldValue::Contacts(_) => ProjectFieldType::Contacts,
            ProjectFieldValue::Url(_) => ProjectFieldType::Url,
            ProjectFieldValue::VideoUrl(_) => ProjectFieldType::VideoUrl,
            ProjectFieldValue::BlogUrl(_) => ProjectFieldType::BlogUrl,
            ProjectFieldValue::TwitterName(_) => ProjectFieldType::TwitterName,
            ProjectFieldValue::FacebookPage(_) => ProjectFieldType::FacebookPage,
            ProjectFieldValue::InstagramPage(_) => ProjectFieldType::InstagramPage,
            ProjectFieldValue::Topics(_) => ProjectFieldType::Topics,
            ProjectFieldValue::FieldsOfScience(_) => ProjectFieldType::FieldsOfScience,
            ProjectFieldValue::Practices(_) => ProjectFieldType::Practices,
            ProjectFieldValue::PracticesHow(_) => ProjectFieldType::PracticesHow,
            ProjectFieldValue::SearchTerms(_) => ProjectFieldType::SearchTerms,
            ProjectFieldValue::Difficulty(_) => ProjectFieldType::Difficulty,
            ProjectFieldValue::Diy(_) => ProjectFieldType::Diy,
            ProjectFieldValue::Audience(_) => ProjectFieldType::Audience,
            ProjectFieldValue::SignupUrl(_) => ProjectFieldType::SignupUrl,
            ProjectFieldValue::PrefixHowToLoggedIn(_) => ProjectFieldType::PrefixHowToLoggedIn,
            ProjectFieldValue::PrefixHowToLoggedOut(_) => ProjectFieldType::PrefixHowToLoggedOut,
            ProjectFieldValue::HowToJoin(_) => ProjectFieldType::HowToJoin,
            ProjectFieldValue::HowtoVideo(_) => ProjectFieldType::HowtoVideo,
            ProjectFieldValue::OutcomesLabel(_) => ProjectFieldType::OutcomesLabel,
            ProjectFieldValue::OutcomesLink(_) => ProjectFieldType::OutcomesLink,
            ProjectFieldValue::TimeRequired(_) => ProjectFieldType::TimeRequired,
            ProjectFieldValue::TrainingMaterials(_) => ProjectFieldType::TrainingMaterials,
            ProjectFieldValue::ClassroomMaterials(_) => ProjectFieldType::ClassroomMaterials,
            ProjectFieldValue::ConsentStatement(_) => ProjectFieldType::ConsentStatement,
            ProjectFieldValue::ConsentStatementDate(_) => ProjectFieldType::ConsentStatementDate,
            ProjectFieldValue::ParticipationTools(_) => ProjectFieldType::ParticipationTools,
            ProjectFieldValue::ParticipationPrompt(_) => ProjectFieldType::ParticipationPrompt,
            ProjectFieldValue::ParticipationConfirmationMessage(_) => {
                ProjectFieldType::ParticipationConfirmationMessage
            }
            ProjectFieldValue::ParticipationNotificationEmail(_) => {
                ProjectFieldType::ParticipationNotificationEmail
            }
            ProjectFieldValue::ParticipationUsage(_) => ProjectFieldType::ParticipationUsage,
            ProjectFieldValue::PartOf(_) => ProjectFieldType::PartOf,
            ProjectFieldValue::AssociatedProjects(_) => ProjectFieldType::AssociatedProjects,
            ProjectFieldValue::TotalExpense(_) => ProjectFieldType::TotalExpense,
            ProjectFieldValue::Materials(_) => ProjectFieldType::Materials,
            ProjectFieldValue::Gear(_) => ProjectFieldType::Gear,
            ProjectFieldValue::MobileAppUrlAndroid(_) => ProjectFieldType::MobileAppUrlAndroid,
            ProjectFieldValue::MobileAppUrlIos(_) => ProjectFieldType::MobileAppUrlIos,
            ProjectFieldValue::MobileAppUrl(_) => ProjectFieldType::MobileAppUrl,
            ProjectFieldValue::LocationType(_) => ProjectFieldType::LocationType,
            ProjectFieldValue::EntirelyOnline(_) => ProjectFieldType::EntirelyOnline,
            ProjectFieldValue::OnDemand(_) => ProjectFieldType::OnDemand,
            ProjectFieldValue::InPerson(_) => ProjectFieldType::InPerson,
            ProjectFieldValue::Indoors(_) => ProjectFieldType::Indoors,
            ProjectFieldValue::Outdoors(_) => ProjectFieldType::Outdoors,
            ProjectFieldValue::Regions(_) => ProjectFieldType::Regions,
            ProjectFieldValue::RegionsDescription(_) => ProjectFieldType::RegionsDescription,
            ProjectFieldValue::Point(_) => ProjectFieldType::Point,
            ProjectFieldValue::Address(_) => ProjectFieldType::Address,
            ProjectFieldValue::City(_) => ProjectFieldType::City,
            ProjectFieldValue::State(_) => ProjectFieldType::State,
            ProjectFieldValue::Zip(_) => ProjectFieldType::Zip,
            ProjectFieldValue::Country(_) => ProjectFieldType::Country,
            ProjectFieldValue::Other(_) => ProjectFieldType::Other,
            ProjectFieldValue::LocationName(_) => ProjectFieldType::LocationName,
            ProjectFieldValue::LocationLanduse(_) => ProjectFieldType::LocationLanduse,
            ProjectFieldValue::LocationDescription(_) => ProjectFieldType::LocationDescription,
            ProjectFieldValue::GeographicScope(_) => ProjectFieldType::GeographicScope,
            ProjectFieldValue::UnitedNationsRegion(_) => ProjectFieldType::UnitedNationsRegion,
            ProjectFieldValue::Begin(_) => ProjectFieldType::Begin,
            ProjectFieldValue::End(_) => ProjectFieldType::End,
            ProjectFieldValue::ProjectTiming(_) => ProjectFieldType::ProjectTiming,
            ProjectFieldValue::ParticipationSchedule(_) => ProjectFieldType::ParticipationSchedule,
            ProjectFieldValue::TimePerParticipation(_) => ProjectFieldType::TimePerParticipation,
            ProjectFieldValue::YearStarted(_) => ProjectFieldType::YearStarted,
            ProjectFieldValue::YearCompleted(_) => ProjectFieldType::YearCompleted,
            ProjectFieldValue::MonthCompleted(_) => ProjectFieldType::MonthCompleted,
            ProjectFieldValue::ParticipationTasks(_) => ProjectFieldType::ParticipationTasks,
            ProjectFieldValue::Activities(_) => ProjectFieldType::Activities,
            ProjectFieldValue::SpecialSkills(_) => ProjectFieldType::SpecialSkills,
            ProjectFieldValue::DataResults(_) => ProjectFieldType::DataResults,
            ProjectFieldValue::DataAggregation(_) => ProjectFieldType::DataAggregation,
            ProjectFieldValue::DataConsent(_) => ProjectFieldType::DataConsent,
            ProjectFieldValue::DataVisualization(_) => ProjectFieldType::DataVisualization,
            ProjectFieldValue::DataAccess(_) => ProjectFieldType::DataAccess,
            ProjectFieldValue::DataQuality(_) => ProjectFieldType::DataQuality,
            ProjectFieldValue::MetadataPublicationUrl(_) => ProjectFieldType::MetadataPublicationUrl,
            ProjectFieldValue::DataPublicationUrl(_) => ProjectFieldType::DataPublicationUrl,
            ProjectFieldValue::DataLicense(_) => ProjectFieldType::DataLicense,
            ProjectFieldValue::IntendedOutcomes(_) => ProjectFieldType::IntendedOutcomes,
            ProjectFieldValue::OutcomeOther(_) => ProjectFieldType::OutcomeOther,
            ProjectFieldValue::UniversityAffiliation(_) => ProjectFieldType::UniversityAffiliation,
            ProjectFieldValue::Sponsors(_) => ProjectFieldType::Sponsors,
            ProjectFieldValue::Stakeholders(_) => ProjectFieldType::Stakeholders,
            ProjectFieldValue::CircuitId(_) => ProjectFieldType::CircuitId,
            ProjectFieldValue::SustainableDevelopmentGoals(_) => {
                ProjectFieldType::SustainableDevelopmentGoals
            }
        }
    }

    // Helper functions for database operations
    async fn save_status(&self, status: ProjectStatus) -> Result<(), Error> {
        
        let project_id_str = self.project_id.to_string();
        let status_str = match status {
            ProjectStatus::Pending => "Pending",
            ProjectStatus::Active => "Active", 
            ProjectStatus::PeriodicallyActive => "Periodically Active",
            ProjectStatus::Hiatus => "Hiatus",
            ProjectStatus::Complete => "Complete",
            ProjectStatus::Hidden => "Hidden",
            ProjectStatus::Expired => "Expired",
        };
        
        sqlx::query!(
            "UPDATE projects SET status = ? WHERE project_id = ?",
            status_str,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_name(&self, name: &str) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        sqlx::query!(
            "UPDATE projects SET name = ? WHERE project_id = ?",
            name,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_description(&self, description: &str) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        sqlx::query!(
            "UPDATE projects SET description = ? WHERE project_id = ?",
            description,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_presenter(&self, presenter: &str) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        sqlx::query!(
            "UPDATE projects SET presenter = ? WHERE project_id = ?",
            presenter,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_goal(&self, goal: &str) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        sqlx::query!(
            "UPDATE projects SET goal = ? WHERE project_id = ?",
            goal,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_task(&self, task: &str) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        sqlx::query!(
            "UPDATE projects SET task = ? WHERE project_id = ?",
            task,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_url(&self, url: &str) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        sqlx::query!(
            "UPDATE projects SET url = ? WHERE project_id = ?",
            url,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_image(&self, image: &str) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        sqlx::query!(
            "UPDATE projects SET image = ? WHERE project_id = ?",
            image,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_admin_notes(&self, notes: &str) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        sqlx::query!(
            "UPDATE projects SET admin_notes = ? WHERE project_id = ?",
            notes,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_draft(&self, draft: bool) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        let draft_int = if draft { 1 } else { 0 };
        
        sqlx::query!(
            "UPDATE projects SET draft = ? WHERE project_id = ?",
            draft_int,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_publish(&self, publish: bool) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        let publish_int = if publish { 1 } else { 0 };
        
        sqlx::query!(
            "UPDATE projects SET publish = ? WHERE project_id = ?",
            publish_int,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_flagged(&self, flagged: bool) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        let flagged_int = if flagged { 1 } else { 0 };
        
        sqlx::query!(
            "UPDATE projects SET flagged = ? WHERE project_id = ?",
            flagged_int,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_duplicate(&self, duplicate: bool) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        let duplicate_int = if duplicate { 1 } else { 0 };
        
        sqlx::query!(
            "UPDATE projects SET duplicate = ? WHERE project_id = ?",
            duplicate_int,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_is_test(&self, is_test: bool) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        let is_test_int = if is_test { 1 } else { 0 };
        
        sqlx::query!(
            "UPDATE projects SET is_test = ? WHERE project_id = ?",
            is_test_int,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_begin(&self, begin: &Option<TimeString>) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        let begin_str = begin.as_ref().map(|ts| ts.to_string());
        
        sqlx::query!(
            "UPDATE projects SET begin = ? WHERE project_id = ?",
            begin_str,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    async fn save_end(&self, end: &Option<TimeString>) -> Result<(), Error> {

        
        let project_id_str = self.project_id.to_string();
        let end_str = end.as_ref().map(|ts| ts.to_string());
        
        sqlx::query!(
            "UPDATE projects SET end = ? WHERE project_id = ?",
            end_str,
            project_id_str
        )
        .execute(db())
        .await?;
        
        Ok(())
    }

    // Helper functions for reading from database
    async fn read_status(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let status_str = sqlx::query_scalar!(
            "SELECT status FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        
        let status = match status_str.as_str() {
            "Pending" => ProjectStatus::Pending,
            "Active" => ProjectStatus::Active,
            "Periodically Active" => ProjectStatus::PeriodicallyActive,
            "Hiatus" => ProjectStatus::Hiatus,
            "Complete" => ProjectStatus::Complete,
            "Hidden" => ProjectStatus::Hidden,
            "Expired" => ProjectStatus::Expired,
            _ => ProjectStatus::Active, // Default fallback
        };
        
        Ok(ProjectField::new(project_id, ProjectFieldValue::Status(status)))
    }

    async fn read_name(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let name = sqlx::query_scalar!(
            "SELECT name FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        
        Ok(ProjectField::new(project_id, ProjectFieldValue::Name(name)))
    }

    async fn read_description(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let description = sqlx::query_scalar!(
            "SELECT description FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        
        Ok(ProjectField::new(project_id, ProjectFieldValue::Description(description)))
    }

    async fn read_presenter(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let presenter = sqlx::query_scalar!(
            "SELECT presenter FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        
        Ok(ProjectField::new(project_id, ProjectFieldValue::Presenter(presenter)))
    }

    async fn read_goal(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let goal = sqlx::query_scalar!(
            "SELECT goal FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        
        Ok(ProjectField::new(project_id, ProjectFieldValue::Goal(goal)))
    }

    async fn read_task(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let task = sqlx::query_scalar!(
            "SELECT task FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        
        Ok(ProjectField::new(project_id, ProjectFieldValue::Task(task)))
    }

    async fn read_url(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let url = sqlx::query_scalar!(
            "SELECT url FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        
        Ok(ProjectField::new(project_id, ProjectFieldValue::Url(url)))
    }

    async fn read_image(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let image = sqlx::query_scalar!(
            "SELECT image FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        
        Ok(ProjectField::new(project_id, ProjectFieldValue::Image(image)))
    }

    async fn read_admin_notes(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let notes = sqlx::query_scalar!(
            "SELECT admin_notes FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        
        Ok(ProjectField::new(project_id, ProjectFieldValue::AdminNotes(notes)))
    }

    async fn read_draft(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let draft = sqlx::query_scalar!(
            "SELECT draft FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Draft(draft)))
    }

    async fn read_publish(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let publish = sqlx::query_scalar!(
            "SELECT publish FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Publish(publish.unwrap_or(false))))
    }

    async fn read_flagged(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let flagged = sqlx::query_scalar!(
            "SELECT flagged FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Flagged(flagged)))
    }

    async fn read_duplicate(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let duplicate = sqlx::query_scalar!(
            "SELECT duplicate FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Duplicate(duplicate)))
    }

    async fn read_is_test(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let is_test = sqlx::query_scalar!(
            "SELECT is_test FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::IsTest(is_test)))
    }

    async fn read_begin(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let begin_str = sqlx::query_scalar!(
            "SELECT begin FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        
        let begin = begin_str.and_then(|s| TimeString::try_from(s).ok());
        Ok(ProjectField::new(project_id, ProjectFieldValue::Begin(begin)))
    }

    async fn read_end(project_id: Ulid) -> Result<Self, Error> {

        
        let project_id_str = project_id.to_string();
        let end_str = sqlx::query_scalar!(
            "SELECT end FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        
        let end = end_str.and_then(|s| TimeString::try_from(s).ok());
        Ok(ProjectField::new(project_id, ProjectFieldValue::End(end)))
    }

    async fn read_authoritative(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let authoritative = sqlx::query_scalar!(
            "SELECT authoritative FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Authoritative(authoritative)))
    }

    async fn read_wild_hope(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let wild_hope = sqlx::query_scalar!(
            "SELECT wild_hope FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::WildHope(wild_hope)))
    }

    async fn read_exchange(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let exchange = sqlx::query_scalar!(
            "SELECT exchange FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Exchange(exchange)))
    }

    async fn read_origin(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let origin = sqlx::query_scalar!(
            "SELECT origin FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Origin(origin)))
    }

    async fn read_suborigin(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let suborigin = sqlx::query_scalar!(
            "SELECT suborigin FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Suborigin(suborigin)))
    }

    async fn read_guid(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let guid = sqlx::query_scalar!(
            "SELECT guid FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Guid(guid)))
    }

    async fn read_opengraph_description(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let opengraph_description = sqlx::query_scalar!(
            "SELECT opengraph_description FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::OpengraphDescription(opengraph_description)))
    }

    async fn read_is_affiliate(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let is_affiliate = sqlx::query_scalar!(
            "SELECT is_affiliate FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::IsAffiliate(is_affiliate)))
    }

    async fn read_is_inaturalist(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let is_inaturalist = sqlx::query_scalar!(
            "SELECT is_inaturalist FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::IsInaturalist(is_inaturalist)))
    }

    async fn read_welcomed(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let welcomed = sqlx::query_scalar!(
            "SELECT welcomed FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Welcomed(welcomed)))
    }

    async fn read_created(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let created_str = sqlx::query_scalar!(
            "SELECT created FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        let created = TimeString::try_from(created_str)?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Created(created)))
    }

    async fn read_participation_degree(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let participation_degree_str = sqlx::query_scalar!(
            "SELECT participation_degree FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        let participation_degree = match participation_degree_str.as_str() {
            "Co-Created" => ProjectParticipationDegree::CoCreated,
            "Collaborative" => ProjectParticipationDegree::Collaborative,
            "Contributory" => ProjectParticipationDegree::Contributory,
            "Contractual" => ProjectParticipationDegree::Contractual,
            _ => ProjectParticipationDegree::Collegial, // Default
        };
        Ok(ProjectField::new(project_id, ProjectFieldValue::ParticipationDegree(participation_degree)))
    }

    async fn read_participant_instructions(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let participant_instructions = sqlx::query_scalar!(
            "SELECT participant_instructions FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ParticipantInstructions(participant_instructions)))
    }

    async fn read_us_federally_funded(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let us_federally_funded = sqlx::query_scalar!(
            "SELECT us_federally_funded FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::UsFederallyFunded(us_federally_funded)))
    }

    async fn read_image_credit(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let image_credit = sqlx::query_scalar!(
            "SELECT image_credit FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ImageCredit(image_credit)))
    }

    async fn read_how_to_join(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let how_to_join = sqlx::query_scalar!(
            "SELECT how_to_join FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::HowToJoin(how_to_join)))
    }

    async fn read_total_expense(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let total_expense = sqlx::query_scalar!(
            "SELECT total_expense FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::TotalExpense(total_expense.map(|x| x as f32))))
    }

    async fn read_special_skills(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let special_skills = sqlx::query_scalar!(
            "SELECT special_skills FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::SpecialSkills(special_skills)))
    }

    async fn read_country(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let country = sqlx::query_scalar!(
            "SELECT country FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Country(country)))
    }

    async fn read_state(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let state = sqlx::query_scalar!(
            "SELECT state FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::State(state)))
    }

    async fn read_city(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let city = sqlx::query_scalar!(
            "SELECT city FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::City(city)))
    }

    async fn read_zip(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let zip = sqlx::query_scalar!(
            "SELECT zip FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Zip(zip)))
    }

    async fn read_address(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let address = sqlx::query_scalar!(
            "SELECT address FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Address(address)))
    }

    async fn read_location_name(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let location_name = sqlx::query_scalar!(
            "SELECT location_name FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::LocationName(location_name)))
    }

    async fn read_entirely_online(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let entirely_online = sqlx::query_scalar!(
            "SELECT entirely_online FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::EntirelyOnline(entirely_online)))
    }

    async fn read_location_type(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let location_type = sqlx::query_scalar!(
            "SELECT location_type FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::LocationType(location_type)))
    }

    async fn read_point(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let point = sqlx::query_scalar!(
            "SELECT AsGeoJSON(point) as 'point: String' FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Point(point)))
    }

    async fn read_regions(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let regions = sqlx::query_scalar!(
            "SELECT AsGeoJSON(regions) as 'regions: String' FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Regions(regions)))
    }

    async fn read_location_description(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let location_description = sqlx::query_scalar!(
            "SELECT location_description FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::LocationDescription(location_description)))
    }

    async fn read_signup_url(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let signup_url = sqlx::query_scalar!(
            "SELECT signup_url FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::SignupUrl(signup_url)))
    }

    async fn read_outcomes_label(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let outcomes_label = sqlx::query_scalar!(
            "SELECT outcomes_label FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::OutcomesLabel(outcomes_label)))
    }

    async fn read_outcomes_link(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let outcomes_link = sqlx::query_scalar!(
            "SELECT outcomes_link FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::OutcomesLink(outcomes_link)))
    }

    async fn read_video_url(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let video_url = sqlx::query_scalar!(
            "SELECT video_url FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::VideoUrl(video_url)))
    }

    async fn read_blog_url(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let blog_url = sqlx::query_scalar!(
            "SELECT blog_url FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::BlogUrl(blog_url)))
    }

    async fn read_twitter_name(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let twitter_name = sqlx::query_scalar!(
            "SELECT twitter_name FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::TwitterName(twitter_name)))
    }

    async fn read_facebook_page(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let facebook_page = sqlx::query_scalar!(
            "SELECT facebook_page FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::FacebookPage(facebook_page)))
    }

    async fn read_instagram_page(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let instagram_page = sqlx::query_scalar!(
            "SELECT instagram_page FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::InstagramPage(instagram_page)))
    }

    async fn read_practices_how(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let practices_how = sqlx::query_scalar!(
            "SELECT practices_how FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::PracticesHow(practices_how)))
    }

    async fn read_search_terms(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let search_terms = sqlx::query_scalar!(
            "SELECT search_terms FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::SearchTerms(search_terms)))
    }

    async fn read_difficulty(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let difficulty = sqlx::query_scalar!(
            "SELECT difficulty FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Difficulty(difficulty)))
    }

    async fn read_diy(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let diy = sqlx::query_scalar!(
            "SELECT diy FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Diy(diy)))
    }

    async fn read_legacy_v1_project_id(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let legacy_v1_project_id = sqlx::query_scalar!(
            "SELECT legacy_v1_project_id FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::LegacyV1ProjectId(legacy_v1_project_id)))
    }

    async fn read_legacy_v2_project_id(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let legacy_v2_project_id = sqlx::query_scalar!(
            "SELECT legacy_v2_project_id FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::LegacyV2ProjectId(legacy_v2_project_id)))
    }

    async fn read_foreign_id(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let foreign_id = sqlx::query_scalar!(
            "SELECT foreign_id FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ForeignId(foreign_id)))
    }

    async fn read_promotion_time(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let promotion_time_str = sqlx::query_scalar!(
            "SELECT promotion_time FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        let promotion_time = match promotion_time_str.as_str() {
            "Promote at any time" => ProjectPromotionTime::PromoteAtAnyTime,
            "Do not promote" => ProjectPromotionTime::DoNotPromote,
            "Prefer promotion on the target date" => ProjectPromotionTime::PreferOnTargetDate,
            _ => return Err(anyhow::anyhow!("Unknown promotion_time value: {}", promotion_time_str)),
        };
        Ok(ProjectField::new(project_id, ProjectFieldValue::PromotionTime(promotion_time)))
    }

    async fn read_promotion_target(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let promotion_target_str = sqlx::query_scalar!(
            "SELECT promotion_target FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        let promotion_target = match promotion_target_str {
            Some(s) => TimeString::try_from(s).ok(),
            None => None,
        };
        Ok(ProjectField::new(project_id, ProjectFieldValue::PromotionTarget(promotion_target)))
    }

    async fn read_inat_project_id(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let inat_project_id = sqlx::query_scalar!(
            "SELECT inat_project_id FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::InatProjectId(inat_project_id.map(|i| i as i32))))
    }

    async fn read_page_creator_id(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let page_creator_id_str = sqlx::query_scalar!(
            "SELECT page_creator_id FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        let page_creator_id = Ulid::from_string(&page_creator_id_str)
            .map_err(|e| anyhow::anyhow!("Invalid ULID format for page_creator_id: {}", e))?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::PageCreatorId(page_creator_id)))
    }

    async fn read_extra(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let extra = sqlx::query_scalar!(
            "SELECT extra FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Extra(extra)))
    }

    async fn read_participant_instructions_email(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let participant_instructions_email = sqlx::query_scalar!(
            "SELECT participant_instructions_email FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ParticipantInstructionsEmail(participant_instructions_email)))
    }

    async fn read_connect_opaque_url(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let connect_opaque_url = sqlx::query_scalar!(
            "SELECT connect_opaque_url FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ConnectOpaqueUrl(connect_opaque_url)))
    }

    async fn read_connect_opaque_url1(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let connect_opaque_url1 = sqlx::query_scalar!(
            "SELECT connect_opaque_url1 FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ConnectOpaqueUrl1(connect_opaque_url1)))
    }

    async fn read_connect_opaque_url2(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let connect_opaque_url2 = sqlx::query_scalar!(
            "SELECT connect_opaque_url2 FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ConnectOpaqueUrl2(connect_opaque_url2)))
    }

    async fn read_prefix_how_to_logged_in(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let prefix_how_to_logged_in = sqlx::query_scalar!(
            "SELECT prefix_how_to_logged_in FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::PrefixHowToLoggedIn(prefix_how_to_logged_in)))
    }

    async fn read_prefix_how_to_logged_out(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let prefix_how_to_logged_out = sqlx::query_scalar!(
            "SELECT prefix_how_to_logged_out FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::PrefixHowToLoggedOut(prefix_how_to_logged_out)))
    }

    async fn read_howto_video(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let howto_video = sqlx::query_scalar!(
            "SELECT howto_video FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::HowtoVideo(howto_video)))
    }

    async fn read_time_required(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let time_required = sqlx::query_scalar!(
            "SELECT time_required FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::TimeRequired(time_required)))
    }

    async fn read_training_materials(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let training_materials = sqlx::query_scalar!(
            "SELECT training_materials FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::TrainingMaterials(training_materials)))
    }

    async fn read_classroom_materials(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let classroom_materials = sqlx::query_scalar!(
            "SELECT classroom_materials FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ClassroomMaterials(classroom_materials)))
    }

    async fn read_consent_statement(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let consent_statement = sqlx::query_scalar!(
            "SELECT consent_statement FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ConsentStatement(consent_statement)))
    }

    async fn read_consent_statement_date(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let consent_statement_date_str = sqlx::query_scalar!(
            "SELECT consent_statement_date FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        let consent_statement_date = match consent_statement_date_str {
            Some(s) => TimeString::try_from(s).ok(),
            None => None,
        };
        Ok(ProjectField::new(project_id, ProjectFieldValue::ConsentStatementDate(consent_statement_date)))
    }

    async fn read_participation_tools(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let participation_tools = sqlx::query_scalar!(
            "SELECT participation_tools FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ParticipationTools(participation_tools)))
    }

    async fn read_participation_prompt(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let participation_prompt = sqlx::query_scalar!(
            "SELECT participation_prompt FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ParticipationPrompt(participation_prompt)))
    }

    async fn read_participation_confirmation_message(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let participation_confirmation_message = sqlx::query_scalar!(
            "SELECT participation_confirmation_message FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ParticipationConfirmationMessage(participation_confirmation_message)))
    }

    async fn read_participation_notification_email(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let participation_notification_email = sqlx::query_scalar!(
            "SELECT participation_notification_email FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ParticipationNotificationEmail(participation_notification_email)))
    }

    async fn read_participation_usage(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let participation_usage = sqlx::query_scalar!(
            "SELECT participation_usage FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ParticipationUsage(participation_usage)))
    }

    async fn read_part_of(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let part_of_str = sqlx::query_scalar!(
            "SELECT part_of FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        let part_of = match part_of_str {
            Some(s) => Some(Ulid::from_string(&s).map_err(|e| anyhow::anyhow!("Invalid ULID format for part_of: {}", e))?),
            None => None,
        };
        Ok(ProjectField::new(project_id, ProjectFieldValue::PartOf(part_of)))
    }

    async fn read_materials(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let materials = sqlx::query_scalar!(
            "SELECT materials FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Materials(materials)))
    }

    async fn read_gear(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let gear = sqlx::query_scalar!(
            "SELECT gear FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Gear(gear)))
    }

    async fn read_mobile_app_url_android(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let mobile_app_url_android = sqlx::query_scalar!(
            "SELECT mobile_app_url_android FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::MobileAppUrlAndroid(mobile_app_url_android)))
    }

    async fn read_mobile_app_url_ios(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let mobile_app_url_ios = sqlx::query_scalar!(
            "SELECT mobile_app_url_ios FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::MobileAppUrlIos(mobile_app_url_ios)))
    }

    async fn read_mobile_app_url(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let mobile_app_url = sqlx::query_scalar!(
            "SELECT mobile_app_url FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::MobileAppUrl(mobile_app_url)))
    }

    async fn read_on_demand(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let on_demand = sqlx::query_scalar!(
            "SELECT on_demand FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::OnDemand(on_demand)))
    }

    async fn read_in_person(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let in_person = sqlx::query_scalar!(
            "SELECT in_person FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::InPerson(in_person)))
    }

    async fn read_indoors(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let indoors = sqlx::query_scalar!(
            "SELECT indoors FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Indoors(indoors)))
    }

    async fn read_outdoors(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let outdoors = sqlx::query_scalar!(
            "SELECT outdoors FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Outdoors(outdoors)))
    }

    async fn read_regions_description(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let regions_description = sqlx::query_scalar!(
            "SELECT regions_description FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::RegionsDescription(regions_description)))
    }

    async fn read_other(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let other = sqlx::query_scalar!(
            "SELECT other FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::Other(other)))
    }

    async fn read_location_landuse(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let location_landuse = sqlx::query_scalar!(
            "SELECT location_landuse FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::LocationLanduse(location_landuse)))
    }

    async fn read_united_nations_region(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let united_nations_region = sqlx::query_scalar!(
            "SELECT united_nations_region FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::UnitedNationsRegion(united_nations_region)))
    }

    async fn read_project_timing(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let project_timing = sqlx::query_scalar!(
            "SELECT project_timing FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ProjectTiming(project_timing)))
    }

    async fn read_participation_schedule(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let participation_schedule = sqlx::query_scalar!(
            "SELECT participation_schedule FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::ParticipationSchedule(participation_schedule)))
    }

    async fn read_time_per_participation(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let time_per_participation = sqlx::query_scalar!(
            "SELECT time_per_participation FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::TimePerParticipation(time_per_participation)))
    }

    async fn read_year_started(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let year_started = sqlx::query_scalar!(
            "SELECT year_started FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::YearStarted(year_started.map(|i| i as i32))))
    }

    async fn read_year_completed(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let year_completed = sqlx::query_scalar!(
            "SELECT year_completed FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::YearCompleted(year_completed.map(|i| i as i32))))
    }

    async fn read_month_completed(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let month_completed = sqlx::query_scalar!(
            "SELECT month_completed FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::MonthCompleted(month_completed.map(|i| i as i32))))
    }

    async fn read_data_results(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let data_results = sqlx::query_scalar!(
            "SELECT data_results FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::DataResults(data_results)))
    }

    async fn read_data_visualization(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let data_visualization = sqlx::query_scalar!(
            "SELECT data_visualization FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::DataVisualization(data_visualization)))
    }

    async fn read_data_quality(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let data_quality = sqlx::query_scalar!(
            "SELECT data_quality FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::DataQuality(data_quality)))
    }

    async fn read_metadata_publication_url(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let metadata_publication_url = sqlx::query_scalar!(
            "SELECT metadata_publication_url FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::MetadataPublicationUrl(metadata_publication_url)))
    }

    async fn read_data_publication_url(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let data_publication_url = sqlx::query_scalar!(
            "SELECT data_publication_url FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::DataPublicationUrl(data_publication_url)))
    }

    async fn read_data_license(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let data_license = sqlx::query_scalar!(
            "SELECT data_license FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::DataLicense(data_license)))
    }

    async fn read_outcome_other(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let outcome_other = sqlx::query_scalar!(
            "SELECT outcome_other FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::OutcomeOther(outcome_other)))
    }

    async fn read_university_affiliation(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let university_affiliation = sqlx::query_scalar!(
            "SELECT university_affiliation FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::UniversityAffiliation(university_affiliation)))
    }

    async fn read_circuit_id(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let circuit_id = sqlx::query_scalar!(
            "SELECT circuit_id FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::CircuitId(circuit_id)))
    }

    async fn read_sustainable_development_goals(project_id: Ulid) -> Result<Self, Error> {
        let project_id_str = project_id.to_string();
        let sustainable_development_goals = sqlx::query_scalar!(
            "SELECT sustainable_development_goals FROM projects WHERE project_id = ?",
            project_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(ProjectField::new(project_id, ProjectFieldValue::SustainableDevelopmentGoals(sustainable_development_goals)))
    }


    // Batch 1: Basic administrative fields (missing save helpers)
    async fn save_authoritative(&self, authoritative: bool) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET authoritative = ? WHERE project_id = ?", authoritative, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_wild_hope(&self, wild_hope: bool) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET wild_hope = ? WHERE project_id = ?", wild_hope, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_exchange(&self, exchange: &Option<String>) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET exchange = ? WHERE project_id = ?", exchange, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_origin(&self, origin: &str) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET origin = ? WHERE project_id = ?", origin, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_suborigin(&self, suborigin: &str) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET suborigin = ? WHERE project_id = ?", suborigin, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_guid(&self, guid: &str) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET guid = ? WHERE project_id = ?", guid, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_opengraph_description(&self, opengraph_description: &str) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET opengraph_description = ? WHERE project_id = ?", opengraph_description, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_is_affiliate(&self, is_affiliate: bool) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET is_affiliate = ? WHERE project_id = ?", is_affiliate, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_is_inaturalist(&self, is_inaturalist: bool) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET is_inaturalist = ? WHERE project_id = ?", is_inaturalist, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_welcomed(&self, welcomed: bool) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET welcomed = ? WHERE project_id = ?", welcomed, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_created(&self, created: &TimeString) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        let created_str = created.to_string();
        sqlx::query!("UPDATE projects SET created = ? WHERE project_id = ?", created_str, project_id_str).execute(db()).await?;
        Ok(())
    }

    // Basic project fields
    async fn save_participation_degree(&self, participation_degree: ProjectParticipationDegree) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        let degree_str = match participation_degree {
            ProjectParticipationDegree::CoCreated => "Co-Created",
            ProjectParticipationDegree::Collaborative => "Collaborative",
            ProjectParticipationDegree::Contributory => "Contributory",
            ProjectParticipationDegree::Contractual => "Contractual",
            ProjectParticipationDegree::Collegial => "Collegial",
        };
        sqlx::query!("UPDATE projects SET participation_degree = ? WHERE project_id = ?", degree_str, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_participant_instructions(&self, participant_instructions: &str) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET participant_instructions = ? WHERE project_id = ?", participant_instructions, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_us_federally_funded(&self, us_federally_funded: bool) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET us_federally_funded = ? WHERE project_id = ?", us_federally_funded, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_image_credit(&self, image_credit: &str) -> Result<(), Error> {

        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET image_credit = ? WHERE project_id = ?", image_credit, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_how_to_join(&self, how_to_join: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET how_to_join = ? WHERE project_id = ?", how_to_join, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_total_expense(&self, total_expense: &Option<f32>) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET total_expense = ? WHERE project_id = ?", total_expense, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_special_skills(&self, special_skills: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET special_skills = ? WHERE project_id = ?", special_skills, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_country(&self, country: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET country = ? WHERE project_id = ?", country, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_state(&self, state: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET state = ? WHERE project_id = ?", state, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_city(&self, city: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET city = ? WHERE project_id = ?", city, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_zip(&self, zip: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET zip = ? WHERE project_id = ?", zip, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_address(&self, address: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET address = ? WHERE project_id = ?", address, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_location_name(&self, location_name: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET location_name = ? WHERE project_id = ?", location_name, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_entirely_online(&self, entirely_online: bool) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        let entirely_online_int = if entirely_online { 1 } else { 0 };
        sqlx::query!("UPDATE projects SET entirely_online = ? WHERE project_id = ?", entirely_online_int, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_location_type(&self, location_type: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET location_type = ? WHERE project_id = ?", location_type, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_point(&self, point: &Option<String>) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!(
            r#"UPDATE projects SET point = case
                when ?1 is null then null
                when substr(?1, 1, 1) == '{' and substr(?1, -1, 1) == '}' then GeomFromGeoJSON(?1)
                else GeomFromEWKT(?1)
            end WHERE project_id = ?2"#,
            point,
            project_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_regions(&self, regions: &Option<String>) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!(
            r#"UPDATE projects SET regions = case
                when ?1 is null then null
                when substr(?1, 1, 1) == '{' and substr(?1, -1, 1) == '}' then GeomFromGeoJSON(?1)
                else GeomFromEWKT(?1)
            end WHERE project_id = ?2"#,
            regions,
            project_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_location_description(&self, location_description: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET location_description = ? WHERE project_id = ?", location_description, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_signup_url(&self, signup_url: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET signup_url = ? WHERE project_id = ?", signup_url, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_outcomes_label(&self, outcomes_label: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET outcomes_label = ? WHERE project_id = ?", outcomes_label, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_outcomes_link(&self, outcomes_link: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET outcomes_link = ? WHERE project_id = ?", outcomes_link, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_video_url(&self, video_url: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET video_url = ? WHERE project_id = ?", video_url, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_blog_url(&self, blog_url: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET blog_url = ? WHERE project_id = ?", blog_url, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_twitter_name(&self, twitter_name: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET twitter_name = ? WHERE project_id = ?", twitter_name, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_facebook_page(&self, facebook_page: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET facebook_page = ? WHERE project_id = ?", facebook_page, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_instagram_page(&self, instagram_page: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET instagram_page = ? WHERE project_id = ?", instagram_page, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_practices_how(&self, practices_how: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET practices_how = ? WHERE project_id = ?", practices_how, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_search_terms(&self, search_terms: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET search_terms = ? WHERE project_id = ?", search_terms, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_difficulty(&self, difficulty: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET difficulty = ? WHERE project_id = ?", difficulty, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_diy(&self, diy: bool) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET diy = ? WHERE project_id = ?", diy, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_legacy_v1_project_id(&self, legacy_v1_project_id: &Option<i64>) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET legacy_v1_project_id = ? WHERE project_id = ?", legacy_v1_project_id, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_legacy_v2_project_id(&self, legacy_v2_project_id: &Option<i64>) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET legacy_v2_project_id = ? WHERE project_id = ?", legacy_v2_project_id, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_foreign_id(&self, foreign_id: &Option<String>) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET foreign_id = ? WHERE project_id = ?", foreign_id, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_promotion_time(&self, promotion_time: ProjectPromotionTime) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        let promotion_time_str = match promotion_time {
            ProjectPromotionTime::PromoteAtAnyTime => "Promote at any time",
            ProjectPromotionTime::DoNotPromote => "Do not promote", 
            ProjectPromotionTime::PreferOnTargetDate => "Prefer promotion on the target date",
        };
        sqlx::query!("UPDATE projects SET promotion_time = ? WHERE project_id = ?", promotion_time_str, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_promotion_target(&self, promotion_target: &Option<TimeString>) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        let promotion_target_str = promotion_target.as_ref().map(|t| t.to_string());
        sqlx::query!("UPDATE projects SET promotion_target = ? WHERE project_id = ?", promotion_target_str, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_inat_project_id(&self, inat_project_id: &Option<i32>) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET inat_project_id = ? WHERE project_id = ?", inat_project_id, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_page_creator_id(&self, page_creator_id: Ulid) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        let page_creator_id_str = page_creator_id.to_string();
        sqlx::query!("UPDATE projects SET page_creator_id = ? WHERE project_id = ?", page_creator_id_str, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_extra(&self, extra: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET extra = ? WHERE project_id = ?", extra, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_participant_instructions_email(&self, participant_instructions_email: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET participant_instructions_email = ? WHERE project_id = ?", participant_instructions_email, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_connect_opaque_url(&self, connect_opaque_url: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET connect_opaque_url = ? WHERE project_id = ?", connect_opaque_url, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_connect_opaque_url1(&self, connect_opaque_url1: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET connect_opaque_url1 = ? WHERE project_id = ?", connect_opaque_url1, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_connect_opaque_url2(&self, connect_opaque_url2: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET connect_opaque_url2 = ? WHERE project_id = ?", connect_opaque_url2, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_prefix_how_to_logged_in(&self, prefix_how_to_logged_in: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET prefix_how_to_logged_in = ? WHERE project_id = ?", prefix_how_to_logged_in, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_prefix_how_to_logged_out(&self, prefix_how_to_logged_out: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET prefix_how_to_logged_out = ? WHERE project_id = ?", prefix_how_to_logged_out, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_howto_video(&self, howto_video: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET howto_video = ? WHERE project_id = ?", howto_video, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_time_required(&self, time_required: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET time_required = ? WHERE project_id = ?", time_required, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_training_materials(&self, training_materials: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET training_materials = ? WHERE project_id = ?", training_materials, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_classroom_materials(&self, classroom_materials: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET classroom_materials = ? WHERE project_id = ?", classroom_materials, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_consent_statement(&self, consent_statement: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET consent_statement = ? WHERE project_id = ?", consent_statement, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_consent_statement_date(&self, consent_statement_date: &Option<TimeString>) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        let consent_statement_date_str = consent_statement_date.as_ref().map(|t| t.to_string());
        sqlx::query!("UPDATE projects SET consent_statement_date = ? WHERE project_id = ?", consent_statement_date_str, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_participation_tools(&self, participation_tools: bool) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET participation_tools = ? WHERE project_id = ?", participation_tools, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_participation_prompt(&self, participation_prompt: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET participation_prompt = ? WHERE project_id = ?", participation_prompt, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_participation_confirmation_message(&self, participation_confirmation_message: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET participation_confirmation_message = ? WHERE project_id = ?", participation_confirmation_message, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_participation_notification_email(&self, participation_notification_email: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET participation_notification_email = ? WHERE project_id = ?", participation_notification_email, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_participation_usage(&self, participation_usage: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET participation_usage = ? WHERE project_id = ?", participation_usage, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_part_of(&self, part_of: &Option<Ulid>) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        let part_of_str = part_of.map(|u| u.to_string());
        sqlx::query!("UPDATE projects SET part_of = ? WHERE project_id = ?", part_of_str, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_materials(&self, materials: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET materials = ? WHERE project_id = ?", materials, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_gear(&self, gear: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET gear = ? WHERE project_id = ?", gear, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_mobile_app_url_android(&self, mobile_app_url_android: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET mobile_app_url_android = ? WHERE project_id = ?", mobile_app_url_android, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_mobile_app_url_ios(&self, mobile_app_url_ios: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET mobile_app_url_ios = ? WHERE project_id = ?", mobile_app_url_ios, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_mobile_app_url(&self, mobile_app_url: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET mobile_app_url = ? WHERE project_id = ?", mobile_app_url, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_on_demand(&self, on_demand: bool) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET on_demand = ? WHERE project_id = ?", on_demand, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_in_person(&self, in_person: bool) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET in_person = ? WHERE project_id = ?", in_person, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_indoors(&self, indoors: bool) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET indoors = ? WHERE project_id = ?", indoors, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_outdoors(&self, outdoors: bool) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET outdoors = ? WHERE project_id = ?", outdoors, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_regions_description(&self, regions_description: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET regions_description = ? WHERE project_id = ?", regions_description, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_other(&self, other: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET other = ? WHERE project_id = ?", other, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_location_landuse(&self, location_landuse: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET location_landuse = ? WHERE project_id = ?", location_landuse, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_united_nations_region(&self, united_nations_region: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET united_nations_region = ? WHERE project_id = ?", united_nations_region, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_project_timing(&self, project_timing: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET project_timing = ? WHERE project_id = ?", project_timing, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_participation_schedule(&self, participation_schedule: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET participation_schedule = ? WHERE project_id = ?", participation_schedule, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_time_per_participation(&self, time_per_participation: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET time_per_participation = ? WHERE project_id = ?", time_per_participation, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_year_started(&self, year_started: &Option<i32>) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET year_started = ? WHERE project_id = ?", year_started, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_year_completed(&self, year_completed: &Option<i32>) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET year_completed = ? WHERE project_id = ?", year_completed, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_month_completed(&self, month_completed: &Option<i32>) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET month_completed = ? WHERE project_id = ?", month_completed, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_data_results(&self, data_results: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET data_results = ? WHERE project_id = ?", data_results, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_data_visualization(&self, data_visualization: bool) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET data_visualization = ? WHERE project_id = ?", data_visualization, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_data_quality(&self, data_quality: bool) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET data_quality = ? WHERE project_id = ?", data_quality, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_metadata_publication_url(&self, metadata_publication_url: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET metadata_publication_url = ? WHERE project_id = ?", metadata_publication_url, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_data_publication_url(&self, data_publication_url: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET data_publication_url = ? WHERE project_id = ?", data_publication_url, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_data_license(&self, data_license: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET data_license = ? WHERE project_id = ?", data_license, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_outcome_other(&self, outcome_other: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET outcome_other = ? WHERE project_id = ?", outcome_other, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_university_affiliation(&self, university_affiliation: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET university_affiliation = ? WHERE project_id = ?", university_affiliation, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_circuit_id(&self, circuit_id: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET circuit_id = ? WHERE project_id = ?", circuit_id, project_id_str).execute(db()).await?;
        Ok(())
    }

    async fn save_sustainable_development_goals(&self, sustainable_development_goals: &str) -> Result<(), Error> {
        let project_id_str = self.project_id.to_string();
        sqlx::query!("UPDATE projects SET sustainable_development_goals = ? WHERE project_id = ?", sustainable_development_goals, project_id_str).execute(db()).await?;
        Ok(())
    }
}

impl LoadStorePeered for ProjectField {
    //type DatabaseKey<'k> = (Ulid, ProjectFieldType);
    type DatabaseKey = (Ulid, ProjectFieldType);

    const GROUP: &str = "project";
    const SPECIFIC: &str = "field";

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        match &self.field {
            ProjectFieldValue::Status(status) => self.save_status(*status).await,
            ProjectFieldValue::Name(name) => self.save_name(name).await,
            ProjectFieldValue::Description(description) => self.save_description(description).await,
            ProjectFieldValue::Presenter(presenter) => self.save_presenter(presenter).await,
            ProjectFieldValue::Goal(goal) => self.save_goal(goal).await,
            ProjectFieldValue::Task(task) => self.save_task(task).await,
            ProjectFieldValue::Url(url) => self.save_url(url).await,
            ProjectFieldValue::Image(image) => self.save_image(image).await,
            ProjectFieldValue::AdminNotes(notes) => self.save_admin_notes(notes).await,
            ProjectFieldValue::Draft(draft) => self.save_draft(*draft).await,
            ProjectFieldValue::Publish(publish) => self.save_publish(*publish).await,
            ProjectFieldValue::Flagged(flagged) => self.save_flagged(*flagged).await,
            ProjectFieldValue::Duplicate(duplicate) => self.save_duplicate(*duplicate).await,
            ProjectFieldValue::IsTest(is_test) => self.save_is_test(*is_test).await,
            ProjectFieldValue::Begin(begin) => self.save_begin(begin).await,
            ProjectFieldValue::End(end) => self.save_end(end).await,
            ProjectFieldValue::Authoritative(authoritative) => self.save_authoritative(*authoritative).await,
            ProjectFieldValue::WildHope(wild_hope) => self.save_wild_hope(*wild_hope).await,
            ProjectFieldValue::Exchange(exchange) => self.save_exchange(exchange).await,
            ProjectFieldValue::Origin(origin) => self.save_origin(origin).await,
            ProjectFieldValue::Suborigin(suborigin) => self.save_suborigin(suborigin).await,
            ProjectFieldValue::Guid(guid) => self.save_guid(guid).await,
            ProjectFieldValue::OpengraphDescription(opengraph_description) => self.save_opengraph_description(opengraph_description).await,
            ProjectFieldValue::IsAffiliate(is_affiliate) => self.save_is_affiliate(*is_affiliate).await,
            ProjectFieldValue::IsInaturalist(is_inaturalist) => self.save_is_inaturalist(*is_inaturalist).await,
            ProjectFieldValue::Welcomed(welcomed) => self.save_welcomed(*welcomed).await,
            ProjectFieldValue::Created(created) => self.save_created(created).await,
            ProjectFieldValue::ParticipationDegree(participation_degree) => self.save_participation_degree(*participation_degree).await,
            ProjectFieldValue::ParticipantInstructions(participant_instructions) => self.save_participant_instructions(participant_instructions).await,
            ProjectFieldValue::UsFederallyFunded(us_federally_funded) => self.save_us_federally_funded(*us_federally_funded).await,
            ProjectFieldValue::ImageCredit(image_credit) => self.save_image_credit(image_credit).await,
            ProjectFieldValue::HowToJoin(how_to_join) => self.save_how_to_join(how_to_join).await,
            ProjectFieldValue::TotalExpense(total_expense) => self.save_total_expense(total_expense).await,
            ProjectFieldValue::SpecialSkills(special_skills) => self.save_special_skills(special_skills).await,
            ProjectFieldValue::Country(country) => self.save_country(country).await,
            ProjectFieldValue::State(state) => self.save_state(state).await,
            ProjectFieldValue::City(city) => self.save_city(city).await,
            ProjectFieldValue::Zip(zip) => self.save_zip(zip).await,
            ProjectFieldValue::Address(address) => self.save_address(address).await,
            ProjectFieldValue::LocationName(location_name) => self.save_location_name(location_name).await,
            ProjectFieldValue::EntirelyOnline(entirely_online) => self.save_entirely_online(*entirely_online).await,
            ProjectFieldValue::LocationType(location_type) => self.save_location_type(location_type).await,
            ProjectFieldValue::Point(point) => self.save_point(point).await,
            ProjectFieldValue::Regions(regions) => self.save_regions(regions).await,
            ProjectFieldValue::LocationDescription(location_description) => self.save_location_description(location_description).await,
            ProjectFieldValue::SignupUrl(signup_url) => self.save_signup_url(signup_url).await,
            ProjectFieldValue::OutcomesLabel(outcomes_label) => self.save_outcomes_label(outcomes_label).await,
            ProjectFieldValue::OutcomesLink(outcomes_link) => self.save_outcomes_link(outcomes_link).await,
            ProjectFieldValue::VideoUrl(video_url) => self.save_video_url(video_url).await,
            ProjectFieldValue::BlogUrl(blog_url) => self.save_blog_url(blog_url).await,
            ProjectFieldValue::TwitterName(twitter_name) => self.save_twitter_name(twitter_name).await,
            ProjectFieldValue::FacebookPage(facebook_page) => self.save_facebook_page(facebook_page).await,
            ProjectFieldValue::InstagramPage(instagram_page) => self.save_instagram_page(instagram_page).await,
            ProjectFieldValue::PracticesHow(practices_how) => self.save_practices_how(practices_how).await,
            ProjectFieldValue::SearchTerms(search_terms) => self.save_search_terms(search_terms).await,
            ProjectFieldValue::Difficulty(difficulty) => self.save_difficulty(difficulty).await,
            ProjectFieldValue::Diy(diy) => self.save_diy(*diy).await,
            ProjectFieldValue::LegacyV1ProjectId(legacy_v1_project_id) => self.save_legacy_v1_project_id(legacy_v1_project_id).await,
            ProjectFieldValue::LegacyV2ProjectId(legacy_v2_project_id) => self.save_legacy_v2_project_id(legacy_v2_project_id).await,
            ProjectFieldValue::ForeignId(foreign_id) => self.save_foreign_id(foreign_id).await,
            ProjectFieldValue::PromotionTime(promotion_time) => self.save_promotion_time(*promotion_time).await,
            ProjectFieldValue::PromotionTarget(promotion_target) => self.save_promotion_target(promotion_target).await,
            ProjectFieldValue::InatProjectId(inat_project_id) => self.save_inat_project_id(inat_project_id).await,
            ProjectFieldValue::PageCreatorId(page_creator_id) => self.save_page_creator_id(*page_creator_id).await,
            ProjectFieldValue::Extra(extra) => self.save_extra(extra).await,
            ProjectFieldValue::ParticipantInstructionsEmail(participant_instructions_email) => self.save_participant_instructions_email(participant_instructions_email).await,
            ProjectFieldValue::ConnectOpaqueUrl(connect_opaque_url) => self.save_connect_opaque_url(connect_opaque_url).await,
            ProjectFieldValue::ConnectOpaqueUrl1(connect_opaque_url1) => self.save_connect_opaque_url1(connect_opaque_url1).await,
            ProjectFieldValue::ConnectOpaqueUrl2(connect_opaque_url2) => self.save_connect_opaque_url2(connect_opaque_url2).await,
            ProjectFieldValue::PrefixHowToLoggedIn(prefix_how_to_logged_in) => self.save_prefix_how_to_logged_in(prefix_how_to_logged_in).await,
            ProjectFieldValue::PrefixHowToLoggedOut(prefix_how_to_logged_out) => self.save_prefix_how_to_logged_out(prefix_how_to_logged_out).await,
            ProjectFieldValue::HowtoVideo(howto_video) => self.save_howto_video(howto_video).await,
            ProjectFieldValue::TimeRequired(time_required) => self.save_time_required(time_required).await,
            ProjectFieldValue::TrainingMaterials(training_materials) => self.save_training_materials(training_materials).await,
            ProjectFieldValue::ClassroomMaterials(classroom_materials) => self.save_classroom_materials(classroom_materials).await,
            ProjectFieldValue::ConsentStatement(consent_statement) => self.save_consent_statement(consent_statement).await,
            ProjectFieldValue::ConsentStatementDate(consent_statement_date) => self.save_consent_statement_date(consent_statement_date).await,
            ProjectFieldValue::ParticipationTools(participation_tools) => self.save_participation_tools(*participation_tools).await,
            ProjectFieldValue::ParticipationPrompt(participation_prompt) => self.save_participation_prompt(participation_prompt).await,
            ProjectFieldValue::ParticipationConfirmationMessage(participation_confirmation_message) => self.save_participation_confirmation_message(participation_confirmation_message).await,
            ProjectFieldValue::ParticipationNotificationEmail(participation_notification_email) => self.save_participation_notification_email(participation_notification_email).await,
            ProjectFieldValue::ParticipationUsage(participation_usage) => self.save_participation_usage(participation_usage).await,
            ProjectFieldValue::PartOf(part_of) => self.save_part_of(part_of).await,
            ProjectFieldValue::Materials(materials) => self.save_materials(materials).await,
            ProjectFieldValue::Gear(gear) => self.save_gear(gear).await,
            ProjectFieldValue::MobileAppUrlAndroid(mobile_app_url_android) => self.save_mobile_app_url_android(mobile_app_url_android).await,
            ProjectFieldValue::MobileAppUrlIos(mobile_app_url_ios) => self.save_mobile_app_url_ios(mobile_app_url_ios).await,
            ProjectFieldValue::MobileAppUrl(mobile_app_url) => self.save_mobile_app_url(mobile_app_url).await,
            ProjectFieldValue::OnDemand(on_demand) => self.save_on_demand(*on_demand).await,
            ProjectFieldValue::InPerson(in_person) => self.save_in_person(*in_person).await,
            ProjectFieldValue::Indoors(indoors) => self.save_indoors(*indoors).await,
            ProjectFieldValue::Outdoors(outdoors) => self.save_outdoors(*outdoors).await,
            ProjectFieldValue::RegionsDescription(regions_description) => self.save_regions_description(regions_description).await,
            ProjectFieldValue::Other(other) => self.save_other(other).await,
            ProjectFieldValue::LocationLanduse(location_landuse) => self.save_location_landuse(location_landuse).await,
            ProjectFieldValue::UnitedNationsRegion(united_nations_region) => self.save_united_nations_region(united_nations_region).await,
            ProjectFieldValue::ProjectTiming(project_timing) => self.save_project_timing(project_timing).await,
            ProjectFieldValue::ParticipationSchedule(participation_schedule) => self.save_participation_schedule(participation_schedule).await,
            ProjectFieldValue::TimePerParticipation(time_per_participation) => self.save_time_per_participation(time_per_participation).await,
            ProjectFieldValue::YearStarted(year_started) => self.save_year_started(year_started).await,
            ProjectFieldValue::YearCompleted(year_completed) => self.save_year_completed(year_completed).await,
            ProjectFieldValue::MonthCompleted(month_completed) => self.save_month_completed(month_completed).await,
            ProjectFieldValue::DataResults(data_results) => self.save_data_results(data_results).await,
            ProjectFieldValue::DataVisualization(data_visualization) => self.save_data_visualization(*data_visualization).await,
            ProjectFieldValue::DataQuality(data_quality) => self.save_data_quality(*data_quality).await,
            ProjectFieldValue::MetadataPublicationUrl(metadata_publication_url) => self.save_metadata_publication_url(metadata_publication_url).await,
            ProjectFieldValue::DataPublicationUrl(data_publication_url) => self.save_data_publication_url(data_publication_url).await,
            ProjectFieldValue::DataLicense(data_license) => self.save_data_license(data_license).await,
            ProjectFieldValue::OutcomeOther(outcome_other) => self.save_outcome_other(outcome_other).await,
            ProjectFieldValue::UniversityAffiliation(university_affiliation) => self.save_university_affiliation(university_affiliation).await,
            ProjectFieldValue::CircuitId(circuit_id) => self.save_circuit_id(circuit_id).await,
            ProjectFieldValue::SustainableDevelopmentGoals(sustainable_development_goals) => self.save_sustainable_development_goals(sustainable_development_goals).await,
            _ => {
                // For field types not yet implemented, return an error
                Err(anyhow::anyhow!("ProjectFieldValue type {:?} not yet implemented for database save", self.field_type()))
            }
        }
    }

    #[tracing::instrument]
    //async fn read_from_database<'k>(key: Self::DatabaseKey<'k>) -> Result<Self, Error> {
    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, Error> {
        let (project_id, field_type) = key;
        
        match field_type {
            ProjectFieldType::Status => Self::read_status(project_id).await,
            ProjectFieldType::Name => Self::read_name(project_id).await,
            ProjectFieldType::Description => Self::read_description(project_id).await,
            ProjectFieldType::Presenter => Self::read_presenter(project_id).await,
            ProjectFieldType::Goal => Self::read_goal(project_id).await,
            ProjectFieldType::Task => Self::read_task(project_id).await,
            ProjectFieldType::Url => Self::read_url(project_id).await,
            ProjectFieldType::Image => Self::read_image(project_id).await,
            ProjectFieldType::AdminNotes => Self::read_admin_notes(project_id).await,
            ProjectFieldType::Draft => Self::read_draft(project_id).await,
            ProjectFieldType::Publish => Self::read_publish(project_id).await,
            ProjectFieldType::Flagged => Self::read_flagged(project_id).await,
            ProjectFieldType::Duplicate => Self::read_duplicate(project_id).await,
            ProjectFieldType::IsTest => Self::read_is_test(project_id).await,
            ProjectFieldType::Begin => Self::read_begin(project_id).await,
            ProjectFieldType::End => Self::read_end(project_id).await,
            ProjectFieldType::Authoritative => Self::read_authoritative(project_id).await,
            ProjectFieldType::WildHope => Self::read_wild_hope(project_id).await,
            ProjectFieldType::Exchange => Self::read_exchange(project_id).await,
            ProjectFieldType::Origin => Self::read_origin(project_id).await,
            ProjectFieldType::Suborigin => Self::read_suborigin(project_id).await,
            ProjectFieldType::Guid => Self::read_guid(project_id).await,
            ProjectFieldType::OpengraphDescription => Self::read_opengraph_description(project_id).await,
            ProjectFieldType::IsAffiliate => Self::read_is_affiliate(project_id).await,
            ProjectFieldType::IsInaturalist => Self::read_is_inaturalist(project_id).await,
            ProjectFieldType::Welcomed => Self::read_welcomed(project_id).await,
            ProjectFieldType::Created => Self::read_created(project_id).await,
            ProjectFieldType::ParticipationDegree => Self::read_participation_degree(project_id).await,
            ProjectFieldType::ParticipantInstructions => Self::read_participant_instructions(project_id).await,
            ProjectFieldType::UsFederallyFunded => Self::read_us_federally_funded(project_id).await,
            ProjectFieldType::ImageCredit => Self::read_image_credit(project_id).await,
            ProjectFieldType::HowToJoin => Self::read_how_to_join(project_id).await,
            ProjectFieldType::TotalExpense => Self::read_total_expense(project_id).await,
            ProjectFieldType::SpecialSkills => Self::read_special_skills(project_id).await,
            ProjectFieldType::Country => Self::read_country(project_id).await,
            ProjectFieldType::State => Self::read_state(project_id).await,
            ProjectFieldType::City => Self::read_city(project_id).await,
            ProjectFieldType::Zip => Self::read_zip(project_id).await,
            ProjectFieldType::Address => Self::read_address(project_id).await,
            ProjectFieldType::LocationName => Self::read_location_name(project_id).await,
            ProjectFieldType::EntirelyOnline => Self::read_entirely_online(project_id).await,
            ProjectFieldType::LocationType => Self::read_location_type(project_id).await,
            ProjectFieldType::Point => Self::read_point(project_id).await,
            ProjectFieldType::Regions => Self::read_regions(project_id).await,
            ProjectFieldType::LocationDescription => Self::read_location_description(project_id).await,
            ProjectFieldType::SignupUrl => Self::read_signup_url(project_id).await,
            ProjectFieldType::OutcomesLabel => Self::read_outcomes_label(project_id).await,
            ProjectFieldType::OutcomesLink => Self::read_outcomes_link(project_id).await,
            ProjectFieldType::VideoUrl => Self::read_video_url(project_id).await,
            ProjectFieldType::BlogUrl => Self::read_blog_url(project_id).await,
            ProjectFieldType::TwitterName => Self::read_twitter_name(project_id).await,
            ProjectFieldType::FacebookPage => Self::read_facebook_page(project_id).await,
            ProjectFieldType::InstagramPage => Self::read_instagram_page(project_id).await,
            ProjectFieldType::PracticesHow => Self::read_practices_how(project_id).await,
            ProjectFieldType::SearchTerms => Self::read_search_terms(project_id).await,
            ProjectFieldType::Difficulty => Self::read_difficulty(project_id).await,
            ProjectFieldType::Diy => Self::read_diy(project_id).await,
            ProjectFieldType::LegacyV1ProjectId => Self::read_legacy_v1_project_id(project_id).await,
            ProjectFieldType::LegacyV2ProjectId => Self::read_legacy_v2_project_id(project_id).await,
            ProjectFieldType::ForeignId => Self::read_foreign_id(project_id).await,
            ProjectFieldType::PromotionTime => Self::read_promotion_time(project_id).await,
            ProjectFieldType::PromotionTarget => Self::read_promotion_target(project_id).await,
            ProjectFieldType::InatProjectId => Self::read_inat_project_id(project_id).await,
            ProjectFieldType::PageCreatorId => Self::read_page_creator_id(project_id).await,
            ProjectFieldType::Extra => Self::read_extra(project_id).await,
            ProjectFieldType::ParticipantInstructionsEmail => Self::read_participant_instructions_email(project_id).await,
            ProjectFieldType::ConnectOpaqueUrl => Self::read_connect_opaque_url(project_id).await,
            ProjectFieldType::ConnectOpaqueUrl1 => Self::read_connect_opaque_url1(project_id).await,
            ProjectFieldType::ConnectOpaqueUrl2 => Self::read_connect_opaque_url2(project_id).await,
            ProjectFieldType::PrefixHowToLoggedIn => Self::read_prefix_how_to_logged_in(project_id).await,
            ProjectFieldType::PrefixHowToLoggedOut => Self::read_prefix_how_to_logged_out(project_id).await,
            ProjectFieldType::HowtoVideo => Self::read_howto_video(project_id).await,
            ProjectFieldType::TimeRequired => Self::read_time_required(project_id).await,
            ProjectFieldType::TrainingMaterials => Self::read_training_materials(project_id).await,
            ProjectFieldType::ClassroomMaterials => Self::read_classroom_materials(project_id).await,
            ProjectFieldType::ConsentStatement => Self::read_consent_statement(project_id).await,
            ProjectFieldType::ConsentStatementDate => Self::read_consent_statement_date(project_id).await,
            ProjectFieldType::ParticipationTools => Self::read_participation_tools(project_id).await,
            ProjectFieldType::ParticipationPrompt => Self::read_participation_prompt(project_id).await,
            ProjectFieldType::ParticipationConfirmationMessage => Self::read_participation_confirmation_message(project_id).await,
            ProjectFieldType::ParticipationNotificationEmail => Self::read_participation_notification_email(project_id).await,
            ProjectFieldType::ParticipationUsage => Self::read_participation_usage(project_id).await,
            ProjectFieldType::PartOf => Self::read_part_of(project_id).await,
            ProjectFieldType::Materials => Self::read_materials(project_id).await,
            ProjectFieldType::Gear => Self::read_gear(project_id).await,
            ProjectFieldType::MobileAppUrlAndroid => Self::read_mobile_app_url_android(project_id).await,
            ProjectFieldType::MobileAppUrlIos => Self::read_mobile_app_url_ios(project_id).await,
            ProjectFieldType::MobileAppUrl => Self::read_mobile_app_url(project_id).await,
            ProjectFieldType::OnDemand => Self::read_on_demand(project_id).await,
            ProjectFieldType::InPerson => Self::read_in_person(project_id).await,
            ProjectFieldType::Indoors => Self::read_indoors(project_id).await,
            ProjectFieldType::Outdoors => Self::read_outdoors(project_id).await,
            ProjectFieldType::RegionsDescription => Self::read_regions_description(project_id).await,
            ProjectFieldType::Other => Self::read_other(project_id).await,
            ProjectFieldType::LocationLanduse => Self::read_location_landuse(project_id).await,
            ProjectFieldType::UnitedNationsRegion => Self::read_united_nations_region(project_id).await,
            ProjectFieldType::ProjectTiming => Self::read_project_timing(project_id).await,
            ProjectFieldType::ParticipationSchedule => Self::read_participation_schedule(project_id).await,
            ProjectFieldType::TimePerParticipation => Self::read_time_per_participation(project_id).await,
            ProjectFieldType::YearStarted => Self::read_year_started(project_id).await,
            ProjectFieldType::YearCompleted => Self::read_year_completed(project_id).await,
            ProjectFieldType::MonthCompleted => Self::read_month_completed(project_id).await,
            ProjectFieldType::DataResults => Self::read_data_results(project_id).await,
            ProjectFieldType::DataVisualization => Self::read_data_visualization(project_id).await,
            ProjectFieldType::DataQuality => Self::read_data_quality(project_id).await,
            ProjectFieldType::MetadataPublicationUrl => Self::read_metadata_publication_url(project_id).await,
            ProjectFieldType::DataPublicationUrl => Self::read_data_publication_url(project_id).await,
            ProjectFieldType::DataLicense => Self::read_data_license(project_id).await,
            ProjectFieldType::OutcomeOther => Self::read_outcome_other(project_id).await,
            ProjectFieldType::UniversityAffiliation => Self::read_university_affiliation(project_id).await,
            ProjectFieldType::CircuitId => Self::read_circuit_id(project_id).await,
            ProjectFieldType::SustainableDevelopmentGoals => Self::read_sustainable_development_goals(project_id).await,
            _ => {
                // For field types not yet implemented, return an error
                Err(anyhow::anyhow!("ProjectFieldType {:?} not yet implemented for database read", field_type))
            }
        }
    }
}
