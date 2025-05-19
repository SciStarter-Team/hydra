use anyhow::Error;
use async_nats::Message;
use common::{nats_headers, service::send_with_headers, timestring::TimeString};
use ulid::Ulid;

use crate::{
    SERVER_ID_STR,
    contact::Contact,
    project::{ProjectPromotionTime, ProjectStatus},
};

#[derive(Clone, Copy)]
pub enum ProjectFieldType {
    LegacyV1ProjectId,
    LegacyV2ProjectId,
    ForeignId,
    Publish,
    Duplicate,
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
    PageManagerIds,
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
pub enum ProjectField {
    LegacyV1ProjectId(Option<i64>),
    LegacyV2ProjectId(Option<i64>),
    ForeignId(Option<String>),
    Publish(bool),
    Duplicate(bool),
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
    Tags(Box<[String]>),
    ParticipationDegree(String),
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
    PartOf(Option<String>),
    AssociatedProjects(Box<[String]>),
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
    Regions(String),
    RegionsDescription(String),
    Point(String),
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
    pub async fn save(&self, project_id: Ulid) -> Result<(), Error> {
        save_local(project_id, self).await?;
        send_with_headers(
            "frontend.project.save.field",
            nats_headers! {
                "Source": SERVER_ID_STR.as_str(),
            },
            (project_id, self),
        )
        .await?;
        Ok(())
    }

    pub async fn load(project_id: Ulid, field: ProjectFieldType) -> Result<ProjectField, Error> {
        match field {
            ProjectFieldType::LegacyV1ProjectId => todo!(),
            ProjectFieldType::LegacyV2ProjectId => todo!(),
            ProjectFieldType::ForeignId => todo!(),
            ProjectFieldType::Publish => todo!(),
            ProjectFieldType::Duplicate => todo!(),
            ProjectFieldType::Flagged => todo!(),
            ProjectFieldType::IsTest => todo!(),
            ProjectFieldType::Status => todo!(),
            ProjectFieldType::PromotionTime => todo!(),
            ProjectFieldType::PromotionTarget => todo!(),
            ProjectFieldType::AdminNotes => todo!(),
            ProjectFieldType::Created => todo!(),
            ProjectFieldType::WildHope => todo!(),
            ProjectFieldType::Exchange => todo!(),
            ProjectFieldType::Origin => todo!(),
            ProjectFieldType::Suborigin => todo!(),
            ProjectFieldType::Guid => todo!(),
            ProjectFieldType::OpengraphDescription => todo!(),
            ProjectFieldType::IsAffiliate => todo!(),
            ProjectFieldType::IsInaturalist => todo!(),
            ProjectFieldType::InatProjectId => todo!(),
            ProjectFieldType::Welcomed => todo!(),
            ProjectFieldType::PageCreatorId => todo!(),
            ProjectFieldType::PageManagerIds => todo!(),
            ProjectFieldType::Extra => todo!(),
            ProjectFieldType::ProjectLastEditor => todo!(),
            ProjectFieldType::ProjectLastEdited => todo!(),
            ProjectFieldType::ScistarterLastEditor => todo!(),
            ProjectFieldType::ScistarterLastEdited => todo!(),
            ProjectFieldType::Draft => todo!(),
            ProjectFieldType::Name => todo!(),
            ProjectFieldType::Tags => todo!(),
            ProjectFieldType::ParticipationDegree => todo!(),
            ProjectFieldType::ParticipantInstructions => todo!(),
            ProjectFieldType::ParticipantInstructionsEmail => todo!(),
            ProjectFieldType::ParticipantFiles => todo!(),
            ProjectFieldType::UsFederallyFunded => todo!(),
            ProjectFieldType::Presenter => todo!(),
            ProjectFieldType::Goal => todo!(),
            ProjectFieldType::Task => todo!(),
            ProjectFieldType::Description => todo!(),
            ProjectFieldType::Image => todo!(),
            ProjectFieldType::ImageCredit => todo!(),
            ProjectFieldType::ConnectOpaqueUrl => todo!(),
            ProjectFieldType::ConnectOpaqueUrl1 => todo!(),
            ProjectFieldType::ConnectOpaqueUrl2 => todo!(),
            ProjectFieldType::Contacts => todo!(),
            ProjectFieldType::Url => todo!(),
            ProjectFieldType::VideoUrl => todo!(),
            ProjectFieldType::BlogUrl => todo!(),
            ProjectFieldType::TwitterName => todo!(),
            ProjectFieldType::FacebookPage => todo!(),
            ProjectFieldType::InstagramPage => todo!(),
            ProjectFieldType::Topics => todo!(),
            ProjectFieldType::FieldsOfScience => todo!(),
            ProjectFieldType::Practices => todo!(),
            ProjectFieldType::PracticesHow => todo!(),
            ProjectFieldType::SearchTerms => todo!(),
            ProjectFieldType::Difficulty => todo!(),
            ProjectFieldType::Diy => todo!(),
            ProjectFieldType::Audience => todo!(),
            ProjectFieldType::SignupUrl => todo!(),
            ProjectFieldType::PrefixHowToLoggedIn => todo!(),
            ProjectFieldType::PrefixHowToLoggedOut => todo!(),
            ProjectFieldType::HowToJoin => todo!(),
            ProjectFieldType::HowtoVideo => todo!(),
            ProjectFieldType::OutcomesLabel => todo!(),
            ProjectFieldType::OutcomesLink => todo!(),
            ProjectFieldType::TimeRequired => todo!(),
            ProjectFieldType::TrainingMaterials => todo!(),
            ProjectFieldType::ClassroomMaterials => todo!(),
            ProjectFieldType::ConsentStatement => todo!(),
            ProjectFieldType::ConsentStatementDate => todo!(),
            ProjectFieldType::ParticipationTools => todo!(),
            ProjectFieldType::ParticipationPrompt => todo!(),
            ProjectFieldType::ParticipationConfirmationMessage => todo!(),
            ProjectFieldType::ParticipationNotificationEmail => todo!(),
            ProjectFieldType::ParticipationUsage => todo!(),
            ProjectFieldType::PartOf => todo!(),
            ProjectFieldType::AssociatedProjects => todo!(),
            ProjectFieldType::TotalExpense => todo!(),
            ProjectFieldType::Materials => todo!(),
            ProjectFieldType::Gear => todo!(),
            ProjectFieldType::MobileAppUrlAndroid => todo!(),
            ProjectFieldType::MobileAppUrlIos => todo!(),
            ProjectFieldType::MobileAppUrl => todo!(),
            ProjectFieldType::LocationType => todo!(),
            ProjectFieldType::EntirelyOnline => todo!(),
            ProjectFieldType::OnDemand => todo!(),
            ProjectFieldType::InPerson => todo!(),
            ProjectFieldType::Indoors => todo!(),
            ProjectFieldType::Outdoors => todo!(),
            ProjectFieldType::Regions => todo!(),
            ProjectFieldType::RegionsDescription => todo!(),
            ProjectFieldType::Point => todo!(),
            ProjectFieldType::Address => todo!(),
            ProjectFieldType::City => todo!(),
            ProjectFieldType::State => todo!(),
            ProjectFieldType::Zip => todo!(),
            ProjectFieldType::Country => todo!(),
            ProjectFieldType::Other => todo!(),
            ProjectFieldType::LocationName => todo!(),
            ProjectFieldType::LocationLanduse => todo!(),
            ProjectFieldType::LocationDescription => todo!(),
            ProjectFieldType::GeographicScope => todo!(),
            ProjectFieldType::UnitedNationsRegion => todo!(),
            ProjectFieldType::Begin => todo!(),
            ProjectFieldType::End => todo!(),
            ProjectFieldType::ProjectTiming => todo!(),
            ProjectFieldType::ParticipationSchedule => todo!(),
            ProjectFieldType::TimePerParticipation => todo!(),
            ProjectFieldType::YearStarted => todo!(),
            ProjectFieldType::YearCompleted => todo!(),
            ProjectFieldType::MonthCompleted => todo!(),
            ProjectFieldType::ParticipationTasks => todo!(),
            ProjectFieldType::Activities => todo!(),
            ProjectFieldType::SpecialSkills => todo!(),
            ProjectFieldType::DataResults => todo!(),
            ProjectFieldType::DataAggregation => todo!(),
            ProjectFieldType::DataConsent => todo!(),
            ProjectFieldType::DataVisualization => todo!(),
            ProjectFieldType::DataAccess => todo!(),
            ProjectFieldType::DataQuality => todo!(),
            ProjectFieldType::MetadataPublicationUrl => todo!(),
            ProjectFieldType::DataPublicationUrl => todo!(),
            ProjectFieldType::DataLicense => todo!(),
            ProjectFieldType::IntendedOutcomes => todo!(),
            ProjectFieldType::OutcomeOther => todo!(),
            ProjectFieldType::UniversityAffiliation => todo!(),
            ProjectFieldType::Sponsors => todo!(),
            ProjectFieldType::Stakeholders => todo!(),
            ProjectFieldType::CircuitId => todo!(),
            ProjectFieldType::SustainableDevelopmentGoals => todo!(),
        }
    }

    pub fn field_type(&self) -> ProjectFieldType {
        match self {
            ProjectField::LegacyV1ProjectId(_) => ProjectFieldType::LegacyV1ProjectId,
            ProjectField::LegacyV2ProjectId(_) => ProjectFieldType::LegacyV2ProjectId,
            ProjectField::ForeignId(_) => ProjectFieldType::ForeignId,
            ProjectField::Publish(_) => ProjectFieldType::Publish,
            ProjectField::Duplicate(_) => ProjectFieldType::Duplicate,
            ProjectField::Flagged(_) => ProjectFieldType::Flagged,
            ProjectField::IsTest(_) => ProjectFieldType::IsTest,
            ProjectField::Status(_) => ProjectFieldType::Status,
            ProjectField::PromotionTime(_) => ProjectFieldType::PromotionTime,
            ProjectField::PromotionTarget(_) => ProjectFieldType::PromotionTarget,
            ProjectField::AdminNotes(_) => ProjectFieldType::AdminNotes,
            ProjectField::Created(_) => ProjectFieldType::Created,
            ProjectField::WildHope(_) => ProjectFieldType::WildHope,
            ProjectField::Exchange(_) => ProjectFieldType::Exchange,
            ProjectField::Origin(_) => ProjectFieldType::Origin,
            ProjectField::Suborigin(_) => ProjectFieldType::Suborigin,
            ProjectField::Guid(_) => ProjectFieldType::Guid,
            ProjectField::OpengraphDescription(_) => ProjectFieldType::OpengraphDescription,
            ProjectField::IsAffiliate(_) => ProjectFieldType::IsAffiliate,
            ProjectField::IsInaturalist(_) => ProjectFieldType::IsInaturalist,
            ProjectField::InatProjectId(_) => ProjectFieldType::InatProjectId,
            ProjectField::Welcomed(_) => ProjectFieldType::Welcomed,
            ProjectField::PageCreatorId(_) => ProjectFieldType::PageCreatorId,
            ProjectField::PageManagerIds(_) => ProjectFieldType::PageManagerIds,
            ProjectField::Extra(_) => ProjectFieldType::Extra,
            ProjectField::ProjectLastEditor(_) => ProjectFieldType::ProjectLastEditor,
            ProjectField::ProjectLastEdited(_) => ProjectFieldType::ProjectLastEdited,
            ProjectField::ScistarterLastEditor(_) => ProjectFieldType::ScistarterLastEditor,
            ProjectField::ScistarterLastEdited(_) => ProjectFieldType::ScistarterLastEdited,
            ProjectField::Draft(_) => ProjectFieldType::Draft,
            ProjectField::Name(_) => ProjectFieldType::Name,
            ProjectField::Tags(_) => ProjectFieldType::Tags,
            ProjectField::ParticipationDegree(_) => ProjectFieldType::ParticipationDegree,
            ProjectField::ParticipantInstructions(_) => ProjectFieldType::ParticipantInstructions,
            ProjectField::ParticipantInstructionsEmail(_) => {
                ProjectFieldType::ParticipantInstructionsEmail
            }
            ProjectField::ParticipantFiles(_) => ProjectFieldType::ParticipantFiles,
            ProjectField::UsFederallyFunded(_) => ProjectFieldType::UsFederallyFunded,
            ProjectField::Presenter(_) => ProjectFieldType::Presenter,
            ProjectField::Goal(_) => ProjectFieldType::Goal,
            ProjectField::Task(_) => ProjectFieldType::Task,
            ProjectField::Description(_) => ProjectFieldType::Description,
            ProjectField::Image(_) => ProjectFieldType::Image,
            ProjectField::ImageCredit(_) => ProjectFieldType::ImageCredit,
            ProjectField::ConnectOpaqueUrl(_) => ProjectFieldType::ConnectOpaqueUrl,
            ProjectField::ConnectOpaqueUrl1(_) => ProjectFieldType::ConnectOpaqueUrl1,
            ProjectField::ConnectOpaqueUrl2(_) => ProjectFieldType::ConnectOpaqueUrl2,
            ProjectField::Contacts(_) => ProjectFieldType::Contacts,
            ProjectField::Url(_) => ProjectFieldType::Url,
            ProjectField::VideoUrl(_) => ProjectFieldType::VideoUrl,
            ProjectField::BlogUrl(_) => ProjectFieldType::BlogUrl,
            ProjectField::TwitterName(_) => ProjectFieldType::TwitterName,
            ProjectField::FacebookPage(_) => ProjectFieldType::FacebookPage,
            ProjectField::InstagramPage(_) => ProjectFieldType::InstagramPage,
            ProjectField::Topics(_) => ProjectFieldType::Topics,
            ProjectField::FieldsOfScience(_) => ProjectFieldType::FieldsOfScience,
            ProjectField::Practices(_) => ProjectFieldType::Practices,
            ProjectField::PracticesHow(_) => ProjectFieldType::PracticesHow,
            ProjectField::SearchTerms(_) => ProjectFieldType::SearchTerms,
            ProjectField::Difficulty(_) => ProjectFieldType::Difficulty,
            ProjectField::Diy(_) => ProjectFieldType::Diy,
            ProjectField::Audience(_) => ProjectFieldType::Audience,
            ProjectField::SignupUrl(_) => ProjectFieldType::SignupUrl,
            ProjectField::PrefixHowToLoggedIn(_) => ProjectFieldType::PrefixHowToLoggedIn,
            ProjectField::PrefixHowToLoggedOut(_) => ProjectFieldType::PrefixHowToLoggedOut,
            ProjectField::HowToJoin(_) => ProjectFieldType::HowToJoin,
            ProjectField::HowtoVideo(_) => ProjectFieldType::HowtoVideo,
            ProjectField::OutcomesLabel(_) => ProjectFieldType::OutcomesLabel,
            ProjectField::OutcomesLink(_) => ProjectFieldType::OutcomesLink,
            ProjectField::TimeRequired(_) => ProjectFieldType::TimeRequired,
            ProjectField::TrainingMaterials(_) => ProjectFieldType::TrainingMaterials,
            ProjectField::ClassroomMaterials(_) => ProjectFieldType::ClassroomMaterials,
            ProjectField::ConsentStatement(_) => ProjectFieldType::ConsentStatement,
            ProjectField::ConsentStatementDate(_) => ProjectFieldType::ConsentStatementDate,
            ProjectField::ParticipationTools(_) => ProjectFieldType::ParticipationTools,
            ProjectField::ParticipationPrompt(_) => ProjectFieldType::ParticipationPrompt,
            ProjectField::ParticipationConfirmationMessage(_) => {
                ProjectFieldType::ParticipationConfirmationMessage
            }
            ProjectField::ParticipationNotificationEmail(_) => {
                ProjectFieldType::ParticipationNotificationEmail
            }
            ProjectField::ParticipationUsage(_) => ProjectFieldType::ParticipationUsage,
            ProjectField::PartOf(_) => ProjectFieldType::PartOf,
            ProjectField::AssociatedProjects(_) => ProjectFieldType::AssociatedProjects,
            ProjectField::TotalExpense(_) => ProjectFieldType::TotalExpense,
            ProjectField::Materials(_) => ProjectFieldType::Materials,
            ProjectField::Gear(_) => ProjectFieldType::Gear,
            ProjectField::MobileAppUrlAndroid(_) => ProjectFieldType::MobileAppUrlAndroid,
            ProjectField::MobileAppUrlIos(_) => ProjectFieldType::MobileAppUrlIos,
            ProjectField::MobileAppUrl(_) => ProjectFieldType::MobileAppUrl,
            ProjectField::LocationType(_) => ProjectFieldType::LocationType,
            ProjectField::EntirelyOnline(_) => ProjectFieldType::EntirelyOnline,
            ProjectField::OnDemand(_) => ProjectFieldType::OnDemand,
            ProjectField::InPerson(_) => ProjectFieldType::InPerson,
            ProjectField::Indoors(_) => ProjectFieldType::Indoors,
            ProjectField::Outdoors(_) => ProjectFieldType::Outdoors,
            ProjectField::Regions(_) => ProjectFieldType::Regions,
            ProjectField::RegionsDescription(_) => ProjectFieldType::RegionsDescription,
            ProjectField::Point(_) => ProjectFieldType::Point,
            ProjectField::Address(_) => ProjectFieldType::Address,
            ProjectField::City(_) => ProjectFieldType::City,
            ProjectField::State(_) => ProjectFieldType::State,
            ProjectField::Zip(_) => ProjectFieldType::Zip,
            ProjectField::Country(_) => ProjectFieldType::Country,
            ProjectField::Other(_) => ProjectFieldType::Other,
            ProjectField::LocationName(_) => ProjectFieldType::LocationName,
            ProjectField::LocationLanduse(_) => ProjectFieldType::LocationLanduse,
            ProjectField::LocationDescription(_) => ProjectFieldType::LocationDescription,
            ProjectField::GeographicScope(_) => ProjectFieldType::GeographicScope,
            ProjectField::UnitedNationsRegion(_) => ProjectFieldType::UnitedNationsRegion,
            ProjectField::Begin(_) => ProjectFieldType::Begin,
            ProjectField::End(_) => ProjectFieldType::End,
            ProjectField::ProjectTiming(_) => ProjectFieldType::ProjectTiming,
            ProjectField::ParticipationSchedule(_) => ProjectFieldType::ParticipationSchedule,
            ProjectField::TimePerParticipation(_) => ProjectFieldType::TimePerParticipation,
            ProjectField::YearStarted(_) => ProjectFieldType::YearStarted,
            ProjectField::YearCompleted(_) => ProjectFieldType::YearCompleted,
            ProjectField::MonthCompleted(_) => ProjectFieldType::MonthCompleted,
            ProjectField::ParticipationTasks(_) => ProjectFieldType::ParticipationTasks,
            ProjectField::Activities(_) => ProjectFieldType::Activities,
            ProjectField::SpecialSkills(_) => ProjectFieldType::SpecialSkills,
            ProjectField::DataResults(_) => ProjectFieldType::DataResults,
            ProjectField::DataAggregation(_) => ProjectFieldType::DataAggregation,
            ProjectField::DataConsent(_) => ProjectFieldType::DataConsent,
            ProjectField::DataVisualization(_) => ProjectFieldType::DataVisualization,
            ProjectField::DataAccess(_) => ProjectFieldType::DataAccess,
            ProjectField::DataQuality(_) => ProjectFieldType::DataQuality,
            ProjectField::MetadataPublicationUrl(_) => ProjectFieldType::MetadataPublicationUrl,
            ProjectField::DataPublicationUrl(_) => ProjectFieldType::DataPublicationUrl,
            ProjectField::DataLicense(_) => ProjectFieldType::DataLicense,
            ProjectField::IntendedOutcomes(_) => ProjectFieldType::IntendedOutcomes,
            ProjectField::OutcomeOther(_) => ProjectFieldType::OutcomeOther,
            ProjectField::UniversityAffiliation(_) => ProjectFieldType::UniversityAffiliation,
            ProjectField::Sponsors(_) => ProjectFieldType::Sponsors,
            ProjectField::Stakeholders(_) => ProjectFieldType::Stakeholders,
            ProjectField::CircuitId(_) => ProjectFieldType::CircuitId,
            ProjectField::SustainableDevelopmentGoals(_) => {
                ProjectFieldType::SustainableDevelopmentGoals
            }
        }
    }
}

pub(super) async fn project_field_save(msg: Message) -> Result<(), Error> {
    if let Some(headers) = msg.headers {
        if headers
            .get("Source")
            .is_some_and(|v| v.as_str() != SERVER_ID_STR.as_str())
        {
            let (project_id, field): (Ulid, ProjectField) = match flexbuffers::from_slice(
                &msg.payload,
            ) {
                Ok(pair) => pair,
                Err(err) => {
                    eprintln!(
                        "Unable to decode message payload to project field record in project_field_save: {}",
                        err
                    );
                    return Ok(());
                }
            };

            save_local(project_id, &field).await?;

            println!("Update accepted");
        } else {
            println!("Update rejected, Source header matches server id");
        }
    }

    Ok(())
}

async fn save_local(project_id: Ulid, field: &ProjectField) -> Result<(), Error> {
    match field {
        ProjectField::LegacyV1ProjectId(value) => {
            save_legacy_v1_project_id_local(project_id, value).await?
        }
        ProjectField::LegacyV2ProjectId(value) => {
            save_legacy_v2_project_id_local(project_id, value).await?
        }
        ProjectField::ForeignId(value) => save_foreign_id_local(project_id, value).await?,
        ProjectField::Publish(value) => save_publish_local(project_id, value).await?,
        ProjectField::Duplicate(value) => save_duplicate_local(project_id, value).await?,
        ProjectField::Flagged(value) => save_flagged_local(project_id, value).await?,
        ProjectField::IsTest(value) => save_is_test_local(project_id, value).await?,
        ProjectField::Status(value) => save_status_local(project_id, value).await?,
        ProjectField::PromotionTime(value) => save_promotion_time_local(project_id, value).await?,
        ProjectField::PromotionTarget(value) => {
            save_promotion_target_local(project_id, value).await?
        }
        ProjectField::AdminNotes(value) => save_admin_notes_local(project_id, value).await?,
        ProjectField::Created(value) => save_created_local(project_id, value).await?,
        ProjectField::WildHope(value) => save_wild_hope_local(project_id, value).await?,
        ProjectField::Exchange(value) => save_exchange_local(project_id, value).await?,
        ProjectField::Origin(value) => save_origin_local(project_id, value).await?,
        ProjectField::Suborigin(value) => save_suborigin_local(project_id, value).await?,
        ProjectField::Guid(value) => save_guid_local(project_id, value).await?,
        ProjectField::OpengraphDescription(value) => {
            save_opengraph_description_local(project_id, value).await?
        }
        ProjectField::IsAffiliate(value) => save_is_affiliate_local(project_id, value).await?,
        ProjectField::IsInaturalist(value) => save_is_inaturalist_local(project_id, value).await?,
        ProjectField::InatProjectId(value) => save_inat_project_id_local(project_id, value).await?,
        ProjectField::Welcomed(value) => save_welcomed_local(project_id, value).await?,
        ProjectField::PageCreatorId(value) => save_page_creator_id_local(project_id, value).await?,
        ProjectField::PageManagerIds(value) => {
            save_page_manager_ids_local(project_id, value).await?
        }
        ProjectField::Extra(value) => save_extra_local(project_id, value).await?,
        ProjectField::ProjectLastEditor(value) => {
            save_project_last_editor_local(project_id, value).await?
        }
        ProjectField::ProjectLastEdited(value) => {
            save_project_last_edited_local(project_id, value).await?
        }
        ProjectField::ScistarterLastEditor(value) => {
            save_scistarter_last_editor_local(project_id, value).await?
        }
        ProjectField::ScistarterLastEdited(value) => {
            save_scistarter_last_edited_local(project_id, value).await?
        }
        ProjectField::Draft(value) => save_draft_local(project_id, value).await?,
        ProjectField::Name(value) => save_name_local(project_id, value).await?,
        ProjectField::Tags(value) => save_tags_local(project_id, value).await?,
        ProjectField::ParticipationDegree(value) => {
            save_participation_degree_local(project_id, value).await?
        }
        ProjectField::ParticipantInstructions(value) => {
            save_participant_instructions_local(project_id, value).await?
        }
        ProjectField::ParticipantInstructionsEmail(value) => {
            save_participant_instructions_email_local(project_id, value).await?
        }
        ProjectField::ParticipantFiles(value) => {
            save_participant_files_local(project_id, value).await?
        }
        ProjectField::UsFederallyFunded(value) => {
            save_us_federally_funded_local(project_id, value).await?
        }
        ProjectField::Presenter(value) => save_presenter_local(project_id, value).await?,
        ProjectField::Goal(value) => save_goal_local(project_id, value).await?,
        ProjectField::Task(value) => save_task_local(project_id, value).await?,
        ProjectField::Description(value) => save_description_local(project_id, value).await?,
        ProjectField::Image(value) => save_image_local(project_id, value).await?,
        ProjectField::ImageCredit(value) => save_image_credit_local(project_id, value).await?,
        ProjectField::ConnectOpaqueUrl(value) => {
            save_connect_opaque_url_local(project_id, value).await?
        }
        ProjectField::ConnectOpaqueUrl1(value) => {
            save_connect_opaque_url1_local(project_id, value).await?
        }
        ProjectField::ConnectOpaqueUrl2(value) => {
            save_connect_opaque_url2_local(project_id, value).await?
        }
        ProjectField::Contacts(value) => save_contacts_local(project_id, value).await?,
        ProjectField::Url(value) => save_url_local(project_id, value).await?,
        ProjectField::VideoUrl(value) => save_video_url_local(project_id, value).await?,
        ProjectField::BlogUrl(value) => save_blog_url_local(project_id, value).await?,
        ProjectField::TwitterName(value) => save_twitter_name_local(project_id, value).await?,
        ProjectField::FacebookPage(value) => save_facebook_page_local(project_id, value).await?,
        ProjectField::InstagramPage(value) => save_instagram_page_local(project_id, value).await?,
        ProjectField::Topics(value) => save_topics_local(project_id, value).await?,
        ProjectField::FieldsOfScience(value) => {
            save_fields_of_science_local(project_id, value).await?
        }
        ProjectField::Practices(value) => save_practices_local(project_id, value).await?,
        ProjectField::PracticesHow(value) => save_practices_how_local(project_id, value).await?,
        ProjectField::SearchTerms(value) => save_search_terms_local(project_id, value).await?,
        ProjectField::Difficulty(value) => save_difficulty_local(project_id, value).await?,
        ProjectField::Diy(value) => save_diy_local(project_id, value).await?,
        ProjectField::Audience(value) => save_audience_local(project_id, value).await?,
        ProjectField::SignupUrl(value) => save_signup_url_local(project_id, value).await?,
        ProjectField::PrefixHowToLoggedIn(value) => {
            save_prefix_how_to_logged_in_local(project_id, value).await?
        }
        ProjectField::PrefixHowToLoggedOut(value) => {
            save_prefix_how_to_logged_out_local(project_id, value).await?
        }
        ProjectField::HowToJoin(value) => save_how_to_join_local(project_id, value).await?,
        ProjectField::HowtoVideo(value) => save_howto_video_local(project_id, value).await?,
        ProjectField::OutcomesLabel(value) => save_outcomes_label_local(project_id, value).await?,
        ProjectField::OutcomesLink(value) => save_outcomes_link_local(project_id, value).await?,
        ProjectField::TimeRequired(value) => save_time_required_local(project_id, value).await?,
        ProjectField::TrainingMaterials(value) => {
            save_training_materials_local(project_id, value).await?
        }
        ProjectField::ClassroomMaterials(value) => {
            save_classroom_materials_local(project_id, value).await?
        }
        ProjectField::ConsentStatement(value) => {
            save_consent_statement_local(project_id, value).await?
        }
        ProjectField::ConsentStatementDate(value) => {
            save_consent_statement_date_local(project_id, value).await?
        }
        ProjectField::ParticipationTools(value) => {
            save_participation_tools_local(project_id, value).await?
        }
        ProjectField::ParticipationPrompt(value) => {
            save_participation_prompt_local(project_id, value).await?
        }
        ProjectField::ParticipationConfirmationMessage(value) => {
            save_participation_confirmation_message_local(project_id, value).await?
        }
        ProjectField::ParticipationNotificationEmail(value) => {
            save_participation_notification_email_local(project_id, value).await?
        }
        ProjectField::ParticipationUsage(value) => {
            save_participation_usage_local(project_id, value).await?
        }
        ProjectField::PartOf(value) => save_part_of_local(project_id, value).await?,
        ProjectField::AssociatedProjects(value) => {
            save_associated_projects_local(project_id, value).await?
        }
        ProjectField::TotalExpense(value) => save_total_expense_local(project_id, value).await?,
        ProjectField::Materials(value) => save_materials_local(project_id, value).await?,
        ProjectField::Gear(value) => save_gear_local(project_id, value).await?,
        ProjectField::MobileAppUrlAndroid(value) => {
            save_mobile_app_url_android_local(project_id, value).await?
        }
        ProjectField::MobileAppUrlIos(value) => {
            save_mobile_app_url_ios_local(project_id, value).await?
        }
        ProjectField::MobileAppUrl(value) => save_mobile_app_url_local(project_id, value).await?,
        ProjectField::LocationType(value) => save_location_type_local(project_id, value).await?,
        ProjectField::EntirelyOnline(value) => {
            save_entirely_online_local(project_id, value).await?
        }
        ProjectField::OnDemand(value) => save_on_demand_local(project_id, value).await?,
        ProjectField::InPerson(value) => save_in_person_local(project_id, value).await?,
        ProjectField::Indoors(value) => save_indoors_local(project_id, value).await?,
        ProjectField::Outdoors(value) => save_outdoors_local(project_id, value).await?,
        ProjectField::Regions(value) => save_regions_local(project_id, value).await?,
        ProjectField::RegionsDescription(value) => {
            save_regions_description_local(project_id, value).await?
        }
        ProjectField::Point(value) => save_point_local(project_id, value).await?,
        ProjectField::Address(value) => save_address_local(project_id, value).await?,
        ProjectField::City(value) => save_city_local(project_id, value).await?,
        ProjectField::State(value) => save_state_local(project_id, value).await?,
        ProjectField::Zip(value) => save_zip_local(project_id, value).await?,
        ProjectField::Country(value) => save_country_local(project_id, value).await?,
        ProjectField::Other(value) => save_other_local(project_id, value).await?,
        ProjectField::LocationName(value) => save_location_name_local(project_id, value).await?,
        ProjectField::LocationLanduse(value) => {
            save_location_landuse_local(project_id, value).await?
        }
        ProjectField::LocationDescription(value) => {
            save_location_description_local(project_id, value).await?
        }
        ProjectField::GeographicScope(value) => {
            save_geographic_scope_local(project_id, value).await?
        }
        ProjectField::UnitedNationsRegion(value) => {
            save_united_nations_region_local(project_id, value).await?
        }
        ProjectField::Begin(value) => save_begin_local(project_id, value).await?,
        ProjectField::End(value) => save_end_local(project_id, value).await?,
        ProjectField::ProjectTiming(value) => save_project_timing_local(project_id, value).await?,
        ProjectField::ParticipationSchedule(value) => {
            save_participation_schedule_local(project_id, value).await?
        }
        ProjectField::TimePerParticipation(value) => {
            save_time_per_participation_local(project_id, value).await?
        }
        ProjectField::YearStarted(value) => save_year_started_local(project_id, value).await?,
        ProjectField::YearCompleted(value) => save_year_completed_local(project_id, value).await?,
        ProjectField::MonthCompleted(value) => {
            save_month_completed_local(project_id, value).await?
        }
        ProjectField::ParticipationTasks(value) => {
            save_participation_tasks_local(project_id, value).await?
        }
        ProjectField::Activities(value) => save_activities_local(project_id, value).await?,
        ProjectField::SpecialSkills(value) => save_special_skills_local(project_id, value).await?,
        ProjectField::DataResults(value) => save_data_results_local(project_id, value).await?,
        ProjectField::DataAggregation(value) => {
            save_data_aggregation_local(project_id, value).await?
        }
        ProjectField::DataConsent(value) => save_data_consent_local(project_id, value).await?,
        ProjectField::DataVisualization(value) => {
            save_data_visualization_local(project_id, value).await?
        }
        ProjectField::DataAccess(value) => save_data_access_local(project_id, value).await?,
        ProjectField::DataQuality(value) => save_data_quality_local(project_id, value).await?,
        ProjectField::MetadataPublicationUrl(value) => {
            save_metadata_publication_url_local(project_id, value).await?
        }
        ProjectField::DataPublicationUrl(value) => {
            save_data_publication_url_local(project_id, value).await?
        }
        ProjectField::DataLicense(value) => save_data_license_local(project_id, value).await?,
        ProjectField::IntendedOutcomes(value) => {
            save_intended_outcomes_local(project_id, value).await?
        }
        ProjectField::OutcomeOther(value) => save_outcome_other_local(project_id, value).await?,
        ProjectField::UniversityAffiliation(value) => {
            save_university_affiliation_local(project_id, value).await?
        }
        ProjectField::Sponsors(value) => save_sponsors_local(project_id, value).await?,
        ProjectField::Stakeholders(value) => save_stakeholders_local(project_id, value).await?,
        ProjectField::CircuitId(value) => save_circuit_id_local(project_id, value).await?,
        ProjectField::SustainableDevelopmentGoals(value) => {
            save_sustainable_development_goals_local(project_id, value).await?
        }
    }

    Ok(())
}

async fn save_legacy_v1_project_id_local(
    _project_id: Ulid,
    _value: &Option<i64>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_legacy_v2_project_id_local(
    _project_id: Ulid,
    _value: &Option<i64>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_foreign_id_local(_project_id: Ulid, _value: &Option<String>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_publish_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_duplicate_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_flagged_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_is_test_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_status_local(_project_id: Ulid, _value: &ProjectStatus) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_promotion_time_local(
    _project_id: Ulid,
    _value: &ProjectPromotionTime,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_promotion_target_local(
    _project_id: Ulid,
    _value: &Option<TimeString>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_admin_notes_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_created_local(_project_id: Ulid, _value: &TimeString) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_wild_hope_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_exchange_local(_project_id: Ulid, _value: &Option<String>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_origin_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_suborigin_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_guid_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_opengraph_description_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_is_affiliate_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_is_inaturalist_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_inat_project_id_local(_project_id: Ulid, _value: &Option<i32>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_welcomed_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_page_creator_id_local(_project_id: Ulid, _value: &Ulid) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_page_manager_ids_local(_project_id: Ulid, _value: &Box<[Ulid]>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_extra_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_project_last_editor_local(
    _project_id: Ulid,
    _value: &Option<Ulid>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_project_last_edited_local(
    _project_id: Ulid,
    _value: &Option<TimeString>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_scistarter_last_editor_local(
    _project_id: Ulid,
    _value: &Option<Ulid>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_scistarter_last_edited_local(
    _project_id: Ulid,
    _value: &Option<TimeString>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_draft_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_name_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_tags_local(_project_id: Ulid, _value: &Box<[String]>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_participation_degree_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_participant_instructions_local(
    _project_id: Ulid,
    _value: &String,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_participant_instructions_email_local(
    _project_id: Ulid,
    _value: &String,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_participant_files_local(
    _project_id: Ulid,
    _value: &Box<[String]>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_us_federally_funded_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_presenter_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_goal_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_task_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_description_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_image_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_image_credit_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_connect_opaque_url_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_connect_opaque_url1_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_connect_opaque_url2_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_contacts_local(_project_id: Ulid, _value: &Box<[Contact]>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_url_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_video_url_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_blog_url_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_twitter_name_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_facebook_page_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_instagram_page_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_topics_local(_project_id: Ulid, _value: &Box<[String]>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_fields_of_science_local(
    _project_id: Ulid,
    _value: &Box<[String]>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_practices_local(_project_id: Ulid, _value: &Box<[String]>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_practices_how_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_search_terms_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_difficulty_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_diy_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_audience_local(_project_id: Ulid, _value: &Box<[String]>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_signup_url_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_prefix_how_to_logged_in_local(
    _project_id: Ulid,
    _value: &String,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_prefix_how_to_logged_out_local(
    _project_id: Ulid,
    _value: &String,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_how_to_join_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_howto_video_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_outcomes_label_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_outcomes_link_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_time_required_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_training_materials_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_classroom_materials_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_consent_statement_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_consent_statement_date_local(
    _project_id: Ulid,
    _value: &Option<TimeString>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_participation_tools_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_participation_prompt_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_participation_confirmation_message_local(
    _project_id: Ulid,
    _value: &String,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_participation_notification_email_local(
    _project_id: Ulid,
    _value: &String,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_participation_usage_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_part_of_local(_project_id: Ulid, _value: &Option<String>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_associated_projects_local(
    _project_id: Ulid,
    _value: &Box<[String]>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_total_expense_local(_project_id: Ulid, _value: &Option<f32>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_materials_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_gear_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_mobile_app_url_android_local(
    _project_id: Ulid,
    _value: &String,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_mobile_app_url_ios_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_mobile_app_url_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_location_type_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_entirely_online_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_on_demand_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_in_person_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_indoors_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_outdoors_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_regions_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_regions_description_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_point_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_address_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_city_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_state_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_zip_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_country_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_other_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_location_name_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_location_landuse_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_location_description_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_geographic_scope_local(
    _project_id: Ulid,
    _value: &Box<[String]>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_united_nations_region_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_begin_local(_project_id: Ulid, _value: &Option<TimeString>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_end_local(_project_id: Ulid, _value: &Option<TimeString>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_project_timing_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_participation_schedule_local(
    _project_id: Ulid,
    _value: &String,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_time_per_participation_local(
    _project_id: Ulid,
    _value: &String,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_year_started_local(_project_id: Ulid, _value: &Option<i32>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_year_completed_local(_project_id: Ulid, _value: &Option<i32>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_month_completed_local(_project_id: Ulid, _value: &Option<i32>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_participation_tasks_local(
    _project_id: Ulid,
    _value: &Box<[String]>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_activities_local(_project_id: Ulid, _value: &Box<[String]>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_special_skills_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_data_results_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_data_aggregation_local(
    _project_id: Ulid,
    _value: &Box<[String]>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_data_consent_local(_project_id: Ulid, _value: &Box<[String]>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_data_visualization_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_data_access_local(_project_id: Ulid, _value: &Box<[String]>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_data_quality_local(_project_id: Ulid, _value: &bool) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_metadata_publication_url_local(
    _project_id: Ulid,
    _value: &String,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_data_publication_url_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_data_license_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_intended_outcomes_local(
    _project_id: Ulid,
    _value: &Box<[String]>,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_outcome_other_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_university_affiliation_local(
    _project_id: Ulid,
    _value: &String,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_sponsors_local(_project_id: Ulid, _value: &Box<[String]>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_stakeholders_local(_project_id: Ulid, _value: &Box<[String]>) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_circuit_id_local(_project_id: Ulid, _value: &String) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}

async fn save_sustainable_development_goals_local(
    _project_id: Ulid,
    _value: &String,
) -> Result<(), Error> {
    let _db = common::shared::db();
    todo!()
}
