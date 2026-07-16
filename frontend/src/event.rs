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

#[derive(
    Clone,
    Copy,
    Default,
    Debug,
    PartialEq,
    Eq,
    sqlx::Type,
    strum::IntoStaticStr,
    serde::Serialize,
    serde::Deserialize,
)]
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
    ActsOfScienceConnected,
    #[default]
    Other,
}

impl EventType {
    /// Every variant, in the order the add-event form offers them.
    pub const ALL: &'static [EventType] = &[
        EventType::BioBlitz,
        EventType::CitizenForum,
        EventType::CityNatureChallenge,
        EventType::Conference,
        EventType::Hackfest,
        EventType::InternationalMonarchMonitoringBlitz,
        EventType::KidFriendly,
        EventType::Library,
        EventType::Megathon,
        EventType::MuseumEvent,
        EventType::NatureMonitoring,
        EventType::Online,
        EventType::ProfessionalDevelopment,
        EventType::ScienceFestival,
        EventType::SocialMediaEvent,
        EventType::TalkOrScienceCafe,
        EventType::Training,
        EventType::Webinar,
        EventType::Ambassador,
        EventType::ActsOfScienceConnected,
        EventType::Other,
    ];

    /// How the type is written in the `events.event_type` column.
    ///
    /// This is the variant name, which is what the `sqlx::Type` encoding
    /// produces and therefore what the existing rows hold. It is deliberately
    /// derived rather than spelled out, so it cannot drift from the encoding
    /// the database actually sees; `event_type_survives_a_database_round_trip`
    /// holds the two together.
    ///
    /// Form controls that write straight back to the column use this for their
    /// option values, and [`EventType::label`] for what the reader sees.
    pub fn storage_name(self) -> &'static str {
        self.into()
    }

    /// The human-facing name.
    pub fn label(self) -> &'static str {
        match self {
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
            EventType::ActsOfScienceConnected => "Acts of Science Connected",
            EventType::Other => "Other",
        }
    }

    /// Recovers a type from its [`EventType::storage_name`].
    pub fn from_storage_name(name: &str) -> Option<EventType> {
        EventType::ALL
            .iter()
            .copied()
            .find(|event_type| event_type.storage_name() == name)
    }
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.label().fmt(f)
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
            "Acts of Science Connected" => Ok(EventType::ActsOfScienceConnected),
            "Other" => Ok(EventType::Other),
            _ => Err(EventError::EventTypeParse),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;
    use sqlx::SqlitePool;

    /// `EventAll` writes `event_type` through the `sqlx::Type` encoding, so it
    /// has to read it back through the same one.
    ///
    /// These drifted apart once already: the read went through `FromStr`, whose
    /// vocabulary is `Display`'s ("professional development") rather than the
    /// stored one ("ProfessionalDevelopment"), and every event whose variant
    /// name differed from its label — roughly a fifth of them — failed to load.
    #[tokio::test]
    async fn event_type_survives_a_database_round_trip() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();

        sqlx::query(r#"CREATE TABLE "t" ("event_type" TEXT NOT NULL)"#)
            .execute(&pool)
            .await
            .unwrap();

        for event_type in EventType::ALL {
            sqlx::query(r#"INSERT INTO "t" ("event_type") VALUES (?)"#)
                .bind(*event_type)
                .execute(&pool)
                .await
                .unwrap();

            // The stored form is the variant name, which is what the ~3,900
            // rows already in "events" hold. Pinning it here means a future
            // rename has to notice that it orphans existing data.
            let stored: String = sqlx::query_scalar(r#"SELECT "event_type" FROM "t""#)
                .fetch_one(&pool)
                .await
                .unwrap();

            check!(stored == format!("{event_type:?}"));

            // Form controls that write back to the column use `storage_name`
            // for their option values, so it has to be the same string the
            // encoding produces.
            check!(stored == event_type.storage_name());

            let read: EventType = sqlx::query_scalar(r#"SELECT "event_type" FROM "t""#)
                .fetch_one(&pool)
                .await
                .unwrap();

            check!(read == *event_type);

            sqlx::query(r#"DELETE FROM "t""#)
                .execute(&pool)
                .await
                .unwrap();
        }
    }

    /// The add-event form renders its options with `Display` and parses the
    /// submission back with `FromStr`, so the two must stay inverses.
    #[test]
    fn event_type_survives_a_display_round_trip() {
        for event_type in EventType::ALL {
            let rendered = event_type.to_string();
            check!(rendered.parse::<EventType>().unwrap() == *event_type);
        }
    }
}
