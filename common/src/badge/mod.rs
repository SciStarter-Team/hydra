//! Badge information and metadata
//!
//! This module provides a well-structured enum-based interface for badge data,
//! replacing the HashMap-based approach from the orrery crate.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alignment {
    pub name: &'static str,
    pub url: &'static str,
    pub description: Option<&'static str>,
    pub framework: Option<&'static str>,
    pub code: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Badge {
    FoundationsOfCitizenScience,
    LibrariesAsHubsForCitizenScience,
    BuildingDataLiteracyThroughCommunityAndCitizenScience,
    TeachingInHigherEducationWithCitizenScience,
    DataEthicsForPractitioners,
    IdealParticipatoryScience,
    OneMillionActsOfScience2024,
    SciStarterAmbassador,
    OneMillionActsOfScience2025,
}

impl Badge {
    /// Returns the badge class identifier
    pub fn badgeclass(&self) -> &'static str {
        match self {
            Badge::FoundationsOfCitizenScience => "6Jg_IwmfQTaJ2kUNePsO5Q",
            Badge::LibrariesAsHubsForCitizenScience => "CWZrsNMPSZSHqKJoz8pLMQ",
            Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience => {
                "e_t41dudSc6dXymoEa8iNw"
            }
            Badge::TeachingInHigherEducationWithCitizenScience => "OP_wrFm7QauZgaXrZY-DhA",
            Badge::DataEthicsForPractitioners => "1OeTfgY1Q-exWqz---kPAQ",
            Badge::IdealParticipatoryScience => "a8cFJfeBTRec05glGM3_9w",
            Badge::OneMillionActsOfScience2024 => "QLLs6Nm9TfqIj3_722yFQg",
            Badge::SciStarterAmbassador => "9csPkdhzQLSZuk-OWREaKg",
            Badge::OneMillionActsOfScience2025 => "U431wPFaTaKwUQO0WhQJRg",
        }
    }

    /// Returns the human-readable name of the badge
    pub fn name(&self) -> &'static str {
        match self {
            Badge::FoundationsOfCitizenScience => "Foundations of Citizen Science",
            Badge::LibrariesAsHubsForCitizenScience => "Libraries as Hubs for Citizen Science",
            Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience => {
                "Building Data Literacy Through Community and Citizen Science"
            }
            Badge::TeachingInHigherEducationWithCitizenScience => {
                "Teaching in Higher Education with Citizen Science"
            }
            Badge::DataEthicsForPractitioners => "Data Ethics for Practitioners",
            Badge::IdealParticipatoryScience => "IDEAL Participatory Science",
            Badge::OneMillionActsOfScience2024 => "One Million Acts of Science 2024",
            Badge::SciStarterAmbassador => "SciStarter Ambassador",
            Badge::OneMillionActsOfScience2025 => "One Million Acts of Science 2025",
        }
    }

    /// Returns the template URL for the badge image
    pub fn template_url(&self) -> &'static str {
        match self {
            Badge::FoundationsOfCitizenScience => "https://static.scistarter.org/img/badge.png",
            Badge::LibrariesAsHubsForCitizenScience => {
                "https://static.scistarter.org/img/badge-libraries-hubs.png"
            }
            Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience => {
                "https://static.scistarter.org/img/badge-data-literacy.png"
            }
            Badge::TeachingInHigherEducationWithCitizenScience => {
                "https://static.scistarter.org/img/badge-tcshe.png"
            }
            Badge::DataEthicsForPractitioners => {
                "https://static.scistarter.org/img/badge-data-ethics.png"
            }
            Badge::IdealParticipatoryScience => "https://static.scistarter.org/img/badge-ideal.png",
            Badge::OneMillionActsOfScience2024 => "https://static.scistarter.org/img/badge-1ma.png",
            Badge::SciStarterAmbassador => "https://static.scistarter.org/img/badge-ambassador.png",
            Badge::OneMillionActsOfScience2025 => {
                "https://static.scistarter.org/img/badge-1ma-2025.png"
            }
        }
    }

    /// Returns the template path for the badge image
    pub fn template_path(&self) -> &'static str {
        match self {
            Badge::FoundationsOfCitizenScience => "badges/badge-foundations.png",
            Badge::LibrariesAsHubsForCitizenScience => "badges/badge-libraries-hubs.png",
            Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience => {
                "badges/badge-data-literacy.png"
            }
            Badge::TeachingInHigherEducationWithCitizenScience => "badges/badge-tcshe.png",
            Badge::DataEthicsForPractitioners => "badges/badge-data-ethics.png",
            Badge::IdealParticipatoryScience => "badges/badge-ideal.png",
            Badge::OneMillionActsOfScience2024 => "badges/badge-1ma.png",
            Badge::SciStarterAmbassador => "badges/badge-ambassador.png",
            Badge::OneMillionActsOfScience2025 => "badges/badge-1ma-2025.png",
        }
    }

    /// Returns the URL where users can earn this badge
    pub fn earn_url(&self) -> Option<&'static str> {
        match self {
            Badge::FoundationsOfCitizenScience => {
                Some("https://scistarter.org/training-foundations")
            }
            Badge::LibrariesAsHubsForCitizenScience => {
                Some("https://scistarter.org/training-library")
            }
            Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience => {
                Some("https://scistarter.org/training-data-literacy")
            }
            Badge::TeachingInHigherEducationWithCitizenScience => {
                Some("https://scistarter.org/training-highered")
            }
            Badge::DataEthicsForPractitioners => Some("https://scistarter.org/training-dataethics"),
            Badge::IdealParticipatoryScience => Some("https://scistarter.org/training-ideal"),
            Badge::OneMillionActsOfScience2024 => None,
            Badge::SciStarterAmbassador => Some("https://scistarter.org/training-ambassador"),
            Badge::OneMillionActsOfScience2025 => None,
        }
    }

    /// Returns the badge class description
    pub fn description(&self) -> &'static str {
        match self {
            Badge::FoundationsOfCitizenScience => {
                "The Foundations of Citizen Science badge is awarded to citizen scientists who have translated their curiosity into impact through their demonstration of foundational citizen science skills and their contribution to advancing scientific research on the SciStarter platform."
            }
            Badge::LibrariesAsHubsForCitizenScience => {
                "This badge recognizes the essential role that libraries play in citizen science as community hubs, information resources, and educational centers that connect patrons with meaningful scientific research opportunities."
            }
            Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience => {
                "This badge certifies the recipient's ability to develop data literacy skills through community engagement and citizen science participation, fostering critical thinking and data-driven decision making."
            }
            Badge::TeachingInHigherEducationWithCitizenScience => {
                "This badge recognizes educators who effectively integrate citizen science into higher education curricula, creating experiential learning opportunities that engage students in authentic scientific research."
            }
            Badge::DataEthicsForPractitioners => {
                "This badge acknowledges practitioners who demonstrate understanding of ethical considerations in data collection, management, and use within citizen science contexts."
            }
            Badge::IdealParticipatoryScience => {
                "This badge recognizes commitment to inclusive, diverse, equitable, accessible, and learner-focused participatory science practices that engage all community members meaningfully."
            }
            Badge::OneMillionActsOfScience2024 => {
                "This badge celebrates participation in the One Million Acts of Science initiative for 2024, contributing to collective scientific discovery and citizen engagement."
            }
            Badge::SciStarterAmbassador => {
                "This badge recognizes outstanding community leaders who promote citizen science participation and serve as ambassadors for the SciStarter platform."
            }
            Badge::OneMillionActsOfScience2025 => {
                "This badge celebrates participation in the One Million Acts of Science initiative for 2025, contributing to collective scientific discovery and citizen engagement."
            }
        }
    }

    /// Returns the criteria URL for earning this badge
    pub fn criteria_url(&self) -> &'static str {
        match self {
            Badge::FoundationsOfCitizenScience => "https://scistarter.org/training-foundations",
            Badge::LibrariesAsHubsForCitizenScience => "https://scistarter.org/training-library",
            Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience => {
                "https://scistarter.org/training-data-literacy"
            }
            Badge::TeachingInHigherEducationWithCitizenScience => {
                "https://scistarter.org/training-highered"
            }
            Badge::DataEthicsForPractitioners => "https://scistarter.org/training-dataethics",
            Badge::IdealParticipatoryScience => "https://scistarter.org/training-ideal",
            Badge::OneMillionActsOfScience2024 => "https://scistarter.org/citizensciencemonth",
            Badge::SciStarterAmbassador => "https://scistarter.org/training-ambassador",
            Badge::OneMillionActsOfScience2025 => "https://scistarter.org/citizensciencemonth",
        }
    }

    /// Returns the detailed criteria text for earning this badge
    pub fn criteria_text(&self) -> &'static str {
        match self {
            Badge::FoundationsOfCitizenScience => include_str!("foundations.md"),
            Badge::LibrariesAsHubsForCitizenScience => include_str!("libraries.md"),
            Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience => {
                include_str!("data_literacy.md")
            }
            Badge::TeachingInHigherEducationWithCitizenScience => include_str!("higher_ed.md"),
            Badge::DataEthicsForPractitioners => include_str!("data_ethics.md"),
            Badge::IdealParticipatoryScience => include_str!("ideal.md"),
            Badge::OneMillionActsOfScience2024 => include_str!("one_million_2024.md"),
            Badge::SciStarterAmbassador => include_str!("ambassador.md"),
            Badge::OneMillionActsOfScience2025 => include_str!("one_million_2025.md"),
        }
    }

    /// Returns the skill alignments for this badge
    pub fn alignments(&self) -> &'static [Alignment] {
        match self {
            Badge::FoundationsOfCitizenScience => &FOUNDATIONS_ALIGNMENTS,
            Badge::LibrariesAsHubsForCitizenScience => &LIBRARIES_ALIGNMENTS,
            Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience => {
                &DATA_LITERACY_ALIGNMENTS
            }
            Badge::TeachingInHigherEducationWithCitizenScience => &TEACHING_ALIGNMENTS,
            Badge::DataEthicsForPractitioners => &DATA_ETHICS_ALIGNMENTS,
            Badge::IdealParticipatoryScience => &IDEAL_ALIGNMENTS,
            Badge::OneMillionActsOfScience2024 => &ONE_MILLION_2024_ALIGNMENTS,
            Badge::SciStarterAmbassador => &AMBASSADOR_ALIGNMENTS,
            Badge::OneMillionActsOfScience2025 => &ONE_MILLION_2025_ALIGNMENTS,
        }
    }

    /// Returns the tags associated with this badge
    pub fn tags(&self) -> &'static [&'static str] {
        match self {
            Badge::FoundationsOfCitizenScience => &["citizen science", "foundations", "training"],
            Badge::LibrariesAsHubsForCitizenScience => {
                &["libraries", "community", "citizen science"]
            }
            Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience => {
                &["data literacy", "community", "citizen science"]
            }
            Badge::TeachingInHigherEducationWithCitizenScience => {
                &["education", "teaching", "higher education"]
            }
            Badge::DataEthicsForPractitioners => &["data ethics", "practitioners", "ethics"],
            Badge::IdealParticipatoryScience => {
                &["inclusive", "diverse", "equitable", "accessible"]
            }
            Badge::OneMillionActsOfScience2024 => &["one million acts", "2024", "participation"],
            Badge::SciStarterAmbassador => &["ambassador", "community", "leadership"],
            Badge::OneMillionActsOfScience2025 => &["one million acts", "2025", "participation"],
        }
    }

    /// Try to create a Badge from a badgeclass string
    pub fn from_badgeclass(badgeclass: &str) -> Option<Self> {
        match badgeclass {
            "6Jg_IwmfQTaJ2kUNePsO5Q" => Some(Badge::FoundationsOfCitizenScience),
            "CWZrsNMPSZSHqKJoz8pLMQ" => Some(Badge::LibrariesAsHubsForCitizenScience),
            "e_t41dudSc6dXymoEa8iNw" => {
                Some(Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience)
            }
            "OP_wrFm7QauZgaXrZY-DhA" => Some(Badge::TeachingInHigherEducationWithCitizenScience),
            "1OeTfgY1Q-exWqz---kPAQ" => Some(Badge::DataEthicsForPractitioners),
            "a8cFJfeBTRec05glGM3_9w" => Some(Badge::IdealParticipatoryScience),
            "QLLs6Nm9TfqIj3_722yFQg" => Some(Badge::OneMillionActsOfScience2024),
            "9csPkdhzQLSZuk-OWREaKg" => Some(Badge::SciStarterAmbassador),
            "U431wPFaTaKwUQO0WhQJRg" => Some(Badge::OneMillionActsOfScience2025),
            _ => None,
        }
    }

    /// Returns all available badges
    pub fn all() -> &'static [Badge] {
        &ALL_BADGES
    }
}

// Static arrays for alignments - these will be populated with data from the original file
static FOUNDATIONS_ALIGNMENTS: [Alignment; 4] = [
    Alignment {
        name: "Data analysis",
        url: "https://lightcast.io/open-skills/skills/KS120GV6C72JMSZKMTD7/data-analysis",
        description: Some(
            "Data Analysis is the ability to inspect, cleanse, transform, and model data with the goal to discover useful information, inform conclusions, and support decision-making.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS120GV6C72JMSZKMTD7"),
    },
    Alignment {
        name: "Data entry",
        url: "https://lightcast.io/open-skills/skills/KS122NY60HCYYDXHQGQN/data-entry",
        description: Some(
            "Data Entry refers to the process of inputting, updating, and managing data in various formats within computer systems or databases.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122NY60HCYYDXHQGQN"),
    },
    Alignment {
        name: "Information gathering",
        url: "https://lightcast.io/open-skills/skills/KS122D66G05F7DQS9K8M/information-gathering",
        description: Some(
            "Information Gathering is the process of collecting, researching, and compiling relevant data from various sources to support decision-making, problem-solving, or knowledge creation.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122D66G05F7DQS9K8M"),
    },
    Alignment {
        name: "Questioning skills",
        url: "https://lightcast.io/open-skills/skills/KS1267S6Y95KTG5FJK5P/questioning-skills",
        description: Some(
            "Questioning Skills refer to the ability to formulate and ask effective questions to gather information, clarify understanding, and stimulate critical thinking.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS1267S6Y95KTG5FJK5P"),
    },
];

// Note: I'll create placeholder alignments for the other badges - these should be populated with actual data
static LIBRARIES_ALIGNMENTS: [Alignment; 0] = [];
static DATA_LITERACY_ALIGNMENTS: [Alignment; 0] = [];
static TEACHING_ALIGNMENTS: [Alignment; 0] = [];
static DATA_ETHICS_ALIGNMENTS: [Alignment; 0] = [];
static IDEAL_ALIGNMENTS: [Alignment; 0] = [];
static ONE_MILLION_2024_ALIGNMENTS: [Alignment; 0] = [];
static AMBASSADOR_ALIGNMENTS: [Alignment; 0] = [];
static ONE_MILLION_2025_ALIGNMENTS: [Alignment; 0] = [];

static ALL_BADGES: [Badge; 9] = [
    Badge::FoundationsOfCitizenScience,
    Badge::LibrariesAsHubsForCitizenScience,
    Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience,
    Badge::TeachingInHigherEducationWithCitizenScience,
    Badge::DataEthicsForPractitioners,
    Badge::IdealParticipatoryScience,
    Badge::OneMillionActsOfScience2024,
    Badge::SciStarterAmbassador,
    Badge::OneMillionActsOfScience2025,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_badge_from_badgeclass() {
        assert_eq!(
            Badge::from_badgeclass("6Jg_IwmfQTaJ2kUNePsO5Q"),
            Some(Badge::FoundationsOfCitizenScience)
        );

        assert_eq!(Badge::from_badgeclass("invalid_id"), None);
    }

    #[test]
    fn test_badge_properties() {
        let badge = Badge::FoundationsOfCitizenScience;

        assert_eq!(badge.badgeclass(), "6Jg_IwmfQTaJ2kUNePsO5Q");
        assert_eq!(badge.name(), "Foundations of Citizen Science");
        assert_eq!(
            badge.earn_url(),
            Some("https://scistarter.org/training-foundations")
        );
        assert!(!badge.description().is_empty());
        assert!(!badge.criteria_text().is_empty());
        assert!(!badge.tags().is_empty());
    }

    #[test]
    fn test_all_badges_have_unique_badgeclass() {
        use std::collections::HashSet;
        let mut badgeclasses = HashSet::new();

        for badge in Badge::all() {
            let badgeclass = badge.badgeclass();
            assert!(
                badgeclasses.insert(badgeclass),
                "Duplicate badgeclass: {}",
                badgeclass
            );
        }
    }

    #[test]
    fn test_roundtrip_badgeclass_conversion() {
        for badge in Badge::all() {
            let badgeclass = badge.badgeclass();
            let converted = Badge::from_badgeclass(badgeclass);
            assert_eq!(
                Some(*badge),
                converted,
                "Failed roundtrip for badge: {:?}",
                badge
            );
        }
    }
}
