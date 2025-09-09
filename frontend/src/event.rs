use std::str::FromStr;

use common::timestring::TimeString;

pub mod all;
pub mod field;

#[derive(Debug, thiserror::Error)]
pub enum EventError {
    #[error("Unable to parse EventType from string")]
    EventTypeParse,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EventInstance {
    pub begin: TimeString,
    pub end: TimeString,
}

#[derive(Clone, Copy, Default, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum EventStatus {
    #[default]
    Active,
    Cancelled,
    Expired,
}

#[derive(Clone, Copy, Default, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum EventType {
    BioBlitz,
    CitizenForum,
    CityNatureChallenge,
    Conference,
    Hackfest,
    InternationalMonarchMonitoringBlitz,
    KidFriendly,
    Library,
    Megathon,
    MuseumEvent,
    NatureMonitoring,
    Online,
    ProfessionalDevelopment,
    ScienceFestival,
    SocialMediaEvent,
    TalkOrScienceCafe,
    Training,
    Webinar,
    Ambassador,
    #[default]
    Other,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            EventType::BioBlitz => "BioBlitz",
            EventType::CitizenForum => "Citizen forum",
            EventType::CityNatureChallenge => "City Nature Challenge",
            EventType::Conference => "Conference",
            EventType::Hackfest => "hackfest",
            EventType::InternationalMonarchMonitoringBlitz => {
                "International Monarch Monitoring Blitz"
            }
            EventType::KidFriendly => "kid-friendly",
            EventType::Library => "library",
            EventType::Megathon => "Megathon",
            EventType::MuseumEvent => "museum event",
            EventType::NatureMonitoring => "Nature monitoring",
            EventType::Online => "Online",
            EventType::ProfessionalDevelopment => "professional development",
            EventType::ScienceFestival => "science festival",
            EventType::SocialMediaEvent => "Social media event",
            EventType::TalkOrScienceCafe => "talk/science cafe",
            EventType::Training => "Training",
            EventType::Webinar => "Webinar",
            EventType::Ambassador => "Ambassador",
            EventType::Other => "Other",
        };

        text.fmt(f)
    }
}

impl FromStr for EventType {
    type Err = EventError;

    #[tracing::instrument]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "BioBlitz" => Ok(EventType::BioBlitz),
            "Citizen forum" => Ok(EventType::CitizenForum),
            "City Nature Challenge" => Ok(EventType::CityNatureChallenge),
            "Conference" => Ok(EventType::Conference),
            "hackfest" => Ok(EventType::Hackfest),
            "International Monarch Monitoring Blitz" => {
                Ok(EventType::InternationalMonarchMonitoringBlitz)
            }
            "kid-friendly" => Ok(EventType::KidFriendly),
            "library" => Ok(EventType::Library),
            "Megathon" => Ok(EventType::Megathon),
            "museum event" => Ok(EventType::MuseumEvent),
            "Nature monitoring" => Ok(EventType::NatureMonitoring),
            "Online" => Ok(EventType::Online),
            "professional development" => Ok(EventType::ProfessionalDevelopment),
            "science festival" => Ok(EventType::ScienceFestival),
            "Social media event" => Ok(EventType::SocialMediaEvent),
            "talk/science cafe" => Ok(EventType::TalkOrScienceCafe),
            "Training" => Ok(EventType::Training),
            "Webinar" => Ok(EventType::Webinar),
            "Ambassador" => Ok(EventType::Ambassador),
            "Other" => Ok(EventType::Other),
            _ => Err(EventError::EventTypeParse),
        }
    }
}
