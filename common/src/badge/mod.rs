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
    OneMillionActsOfScience2026,
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
            Badge::OneMillionActsOfScience2026 => "01KEYVHY8RXNGXVACTR9TY02AD",
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
            Badge::OneMillionActsOfScience2026 => "One Million Acts of Science 2026",
        }
    }

    /// Returns the template URL for the badge image
    pub fn template_url(&self) -> &'static str {
        match self {
            Badge::FoundationsOfCitizenScience => {
                concat!(
                    "https://",
                    env!("DOMAIN"),
                    "/-static/img/badges/badge-foundations.png"
                )
            }
            Badge::LibrariesAsHubsForCitizenScience => {
                concat!(
                    "https://",
                    env!("DOMAIN"),
                    "/-static/img/badges/badge-libraries-hubs.png"
                )
            }
            Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience => {
                concat!(
                    "https://",
                    env!("DOMAIN"),
                    "/-static/img/badges/badge-data-literacy.png"
                )
            }
            Badge::TeachingInHigherEducationWithCitizenScience => {
                concat!(
                    "https://",
                    env!("DOMAIN"),
                    "/-static/img/badges/badge-tcshe.png"
                )
            }
            Badge::DataEthicsForPractitioners => {
                concat!(
                    "https://",
                    env!("DOMAIN"),
                    "/-static/img/badges/badge-data-ethics.png"
                )
            }
            Badge::IdealParticipatoryScience => {
                concat!(
                    "https://",
                    env!("DOMAIN"),
                    "/-static/img/badges/badge-ideal.png"
                )
            }
            Badge::OneMillionActsOfScience2024 => {
                concat!(
                    "https://",
                    env!("DOMAIN"),
                    "/-static/img/badges/badge-1ma.png"
                )
            }
            Badge::SciStarterAmbassador => {
                concat!(
                    "https://",
                    env!("DOMAIN"),
                    "/-static/img/badges/badge-ambassador.png"
                )
            }
            Badge::OneMillionActsOfScience2025 => {
                concat!(
                    "https://",
                    env!("DOMAIN"),
                    "/-static/img/badges/badge-1ma-2025.png"
                )
            }
            Badge::OneMillionActsOfScience2026 => {
                concat!(
                    "https://",
                    env!("DOMAIN"),
                    "/-static/img/badges/badge-1ma-2025.png"
                )
            }
        }
    }

    /// Returns the template path for the badge image
    pub fn template_path(&self) -> &'static str {
        match self {
            Badge::FoundationsOfCitizenScience => "img/badges/badge-foundations.png",
            Badge::LibrariesAsHubsForCitizenScience => "img/badges/badge-libraries-hubs.png",
            Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience => {
                "img/badges/badge-data-literacy.png"
            }
            Badge::TeachingInHigherEducationWithCitizenScience => "img/badges/badge-tcshe.png",
            Badge::DataEthicsForPractitioners => "img/badges/badge-data-ethics.png",
            Badge::IdealParticipatoryScience => "img/badges/badge-ideal.png",
            Badge::OneMillionActsOfScience2024 => "img/badges/badge-1ma.png",
            Badge::SciStarterAmbassador => "img/badges/badge-ambassador.png",
            Badge::OneMillionActsOfScience2025 => "img/badges/badge-1ma-2025.png",
            Badge::OneMillionActsOfScience2026 => "img/badges/badge-1ma-2025.png",
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
            Badge::OneMillionActsOfScience2026 => None,
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
            Badge::OneMillionActsOfScience2026 => {
                "This badge celebrates participation in the 2.50 Million Acts of Science initiative for 2026, contributing to collective scientific discovery and citizen engagement."
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
            Badge::OneMillionActsOfScience2026 => "https://scistarter.org/citizensciencemonth",
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
            Badge::OneMillionActsOfScience2026 => include_str!("one_million_2025.md"),
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
            Badge::OneMillionActsOfScience2026 => &ONE_MILLION_2025_ALIGNMENTS,
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
            Badge::OneMillionActsOfScience2026 => &["one million acts", "2025", "participation"],
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
            "01KEYVHY8RXNGXVACTR9TY02AD" => Some(Badge::OneMillionActsOfScience2026),
            _ => None,
        }
    }

    /// Returns all available badges
    pub fn all() -> &'static [Badge] {
        &ALL_BADGES
    }
}

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

static LIBRARIES_ALIGNMENTS: [Alignment; 11] = [
    Alignment {
        name: "Evidence-based practice",
        url: "https://lightcast.io/open-skills/skills/KS123987840L8J17862K/evidence-based-practice",
        description: Some(
            "Evidence-Based Practice refers to the systematic approach of integrating the best available research evidence with clinical expertise and patient values to inform decision-making in healthcare and other fields.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS123987840L8J17862K"),
    },
    Alignment {
        name: "Library And Information Services",
        url: "https://lightcast.io/open-skills/skills/KS125RC6CTWJ5S7841WG/library-and-information-services",
        description: Some(
            "Library and Information Services refer to the specialized skills and techniques required to manage, collect, organize, and disseminate information resources to support research, learning, and other objectives.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS125RC6CTWJ5S7841WG"),
    },
    Alignment {
        name: "Library Services",
        url: "https://lightcast.io/open-skills/skills/KS125TG66T8TXK7B2VT3/library-services",
        description: Some(
            "Library services are specialized skills that involve managing, organizing, and preserving information resources, as well as providing access to those resources for individuals and organizations.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS125TG66T8TXK7B2VT3"),
    },
    Alignment {
        name: "Communication",
        url: "https://lightcast.io/open-skills/skills/KS122556LMQ829GZCCRV/communication",
        description: Some(
            "Communication is the ability to effectively interact, convey information, and collaborate with others in a clear and understandable manner.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122556LMQ829GZCCRV"),
    },
    Alignment {
        name: "Information literacy",
        url: "https://lightcast.io/open-skills/skills/KS1254G6X37NS306Y596/information-literacy",
        description: Some(
            "Information literacy is a common skill that involves the ability to recognize, locate, evaluate, and effectively use information.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS1254G6X37NS306Y596"),
    },
    Alignment {
        name: "Interdisciplinary Teaching",
        url: "https://lightcast.io/open-skills/skills/KS125856JH9VGVNY0TBP/interdisciplinary-teaching",
        description: Some(
            "Interdisciplinary Teaching refers to an educational approach that integrates concepts and methods from multiple disciplines to enhance learning experiences.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS125856JH9VGVNY0TBP"),
    },
    Alignment {
        name: "Library Instruction",
        url: "https://lightcast.io/open-skills/skills/KS12100638C964SSQDJD/library-instruction",
        description: Some(
            "Library Instruction refers to the teaching of information literacy skills that enable individuals to effectively locate, evaluate, and use information resources.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS12100638C964SSQDJD"),
    },
    Alignment {
        name: "Lifelong Learning",
        url: "https://lightcast.io/open-skills/skills/KS125TX75XTZZXVJL8B5/lifelong-learning",
        description: Some(
            "Lifelong learning is the ongoing, voluntary, and self-motivated pursuit of knowledge for either personal or professional reasons.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS125TX75XTZZXVJL8B5"),
    },
    Alignment {
        name: "Teaching",
        url: "https://lightcast.io/open-skills/skills/KS120PQ6XD1JWMFQVJNW/teaching",
        description: Some(
            "Teaching is a common skill that involves the transfer of knowledge, skills, and values from one person to another.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS120PQ6XD1JWMFQVJNW"),
    },
    Alignment {
        name: "Training and development",
        url: "https://lightcast.io/open-skills/skills/KS122F96DJHT3ZXB8FCZ/training-and-development",
        description: Some(
            "Training And Development involves designing and implementing educational programs to enhance employees' skills and knowledge.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122F96DJHT3ZXB8FCZ"),
    },
    Alignment {
        name: "ALA's Core Competencies of Librarianship",
        url: "https://www.ala.org/sites/default/files/educationcareers/content/2022%20ALA%20Core%20Competences%20of%20Librarianship_FINAL.pdf",
        description: Some(
            "Libraries and the communities they serve are continually evolving. As such, continuing education, professional growth, and a commitment to lifelong learning are key components of a well-informed library professional.",
        ),
        framework: None,
        code: None,
    },
];

static DATA_LITERACY_ALIGNMENTS: [Alignment; 4] = [
    Alignment {
        name: "Data Literacy",
        url: "https://lightcast.io/open-skills/skills/ES476D9219E938F5CB96/data-literacy",
        description: Some(
            "Data Literacy is the ability to read, understand, analyze and interpret data.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ES476D9219E938F5CB96"),
    },
    Alignment {
        name: "Data-Driven Decision Making",
        url: "https://lightcast.io/open-skills/skills/ESF14A354DDD69B5FBD4/data-driven-decision-making",
        description: Some(
            "Data-Driven Decision Making is the process of utilizing relevant and reliable data to inform and guide decision-making within an organization.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ESF14A354DDD69B5FBD4"),
    },
    Alignment {
        name: "Critical thinking",
        url: "https://lightcast.io/open-skills/skills/KS122J56PZMMFL1SVY1F/critical-thinking",
        description: Some(
            "Critical thinking is a skill that allows individuals to analyze and evaluate information to make sound judgements and decisions.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122J56PZMMFL1SVY1F"),
    },
    Alignment {
        name: "Data analysis",
        url: "https://lightcast.io/open-skills/skills/KS120GV6C72JMSZKMTD7/data-analysis",
        description: Some(
            "Data Analysis is the ability to inspect, cleanse, transform, and model data with the goal to discover useful information, inform conclusions, and support decision-making.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS120GV6C72JMSZKMTD7"),
    },
];

static TEACHING_ALIGNMENTS: [Alignment; 4] = [
    Alignment {
        name: "Constructivism",
        url: "https://lightcast.io/open-skills/skills/KSLWYCD49AGA2P0SY8UE/constructivism",
        description: Some(
            "Constructivism refers to a learning theory that emphasizes the role of learners in actively constructing their own understanding and knowledge through experiences and reflection.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KSLWYCD49AGA2P0SY8UE"),
    },
    Alignment {
        name: "Experiential Learning",
        url: "https://lightcast.io/open-skills/skills/KS123R966Z5R6D81XZK4/experiential-learning",
        description: Some(
            "Experiential learning is a unique educational approach that emphasizes hands-on, practical experience as a means of acquiring knowledge and skills.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS123R966Z5R6D81XZK4"),
    },
    Alignment {
        name: "Experiential Education",
        url: "https://lightcast.io/open-skills/skills/KS123R9698P8RCDZJ7H8/experiential-education",
        description: Some(
            "Experiential education is a specialized approach to learning that emphasizes hands-on, real-world experiences as a primary means of teaching.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS123R9698P8RCDZJ7H8"),
    },
    Alignment {
        name: "Formative And Summative Assessments",
        url: "https://lightcast.io/open-skills/skills/ESAEA6B36CACC973C466/formative-and-summative-assessments",
        description: Some(
            "Formative and summative assessments are crucial components of the learning process that help educators gauge the effectiveness of their teaching practices and track their students' progress.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ESAEA6B36CACC973C466"),
    },
];

static DATA_ETHICS_ALIGNMENTS: [Alignment; 2] = [
    Alignment {
        name: "Data governance",
        url: "https://lightcast.io/open-skills/skills/KS122P378DGNVX4NKQKN/data-governance",
        description: Some(
            "Data Governance is the ability to assess, manage, use, improve, monitor, maintain, and protect organizational information through a system of decision rights and accountabilities for information-related processes.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122P378DGNVX4NKQKN"),
    },
    Alignment {
        name: "Data integrity",
        url: "https://lightcast.io/open-skills/skills/KS122P86NZFH1GP38G15/data-integrity",
        description: Some(
            "Data Integrity is the capability to maintain the accuracy, consistency, and reliability of data throughout its lifecycle, ensuring that the data remains free from errors and inconsistencies.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122P86NZFH1GP38G15"),
    },
];

static IDEAL_ALIGNMENTS: [Alignment; 13] = [
    Alignment {
        name: "Diversity Awareness",
        url: "https://lightcast.io/open-skills/skills/KS123285XWQS1TVY0YJ4/diversity-awareness",
        description: Some(
            "Diversity awareness is an important skill to have in today's global and diverse society. It involves recognizing and respecting differences among people, including differences in race, ethnicity, gender, sexual orientation, religion, and culture.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS123285XWQS1TVY0YJ4"),
    },
    Alignment {
        name: "Leadership",
        url: "https://lightcast.io/open-skills/skills/KS124JB619VXG6RQ810C/leadership",
        description: Some(
            "Leadership is a skill that is common among successful individuals in various fields. It involves the ability to motivate and guide a team towards achieving common goals.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS124JB619VXG6RQ810C"),
    },
    Alignment {
        name: "Self-awareness",
        url: "https://lightcast.io/open-skills/skills/ES88C410DA4EE6A2FE77/self-awareness",
        description: Some(
            "Self-awareness is the ability to understand and be mindful of your own thoughts, emotions, and behaviors.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ES88C410DA4EE6A2FE77"),
    },
    Alignment {
        name: "Self-Directed Learning",
        url: "https://lightcast.io/open-skills/skills/ES2F89D25BA6FFC32FA9/self-directed-learning",
        description: Some(
            "Self-Directed Learning refers to the process in which individuals take the initiative to identify their learning needs, set goals, and seek out resources to acquire knowledge and skills independently.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ES2F89D25BA6FFC32FA9"),
    },
    Alignment {
        name: "Decision making",
        url: "https://lightcast.io/open-skills/skills/KS121W46CJXQ5ZQ506MS/decision-making",
        description: Some(
            "Decision making is the process of choosing between two or more options to reach a desired outcome.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS121W46CJXQ5ZQ506MS"),
    },
    Alignment {
        name: "Intercultural Competence",
        url: "https://lightcast.io/open-skills/skills/KS1258479NCTHZ20PFDH/intercultural-competence",
        description: Some(
            "Intercultural competence is the ability to communicate effectively and appropriately with people from different cultures.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS1258479NCTHZ20PFDH"),
    },
    Alignment {
        name: "Stakeholder engagement",
        url: "https://lightcast.io/open-skills/skills/KS440XG653B0ZK3926JF/stakeholder-engagement",
        description: Some(
            "Stakeholder Engagement is a critical skill that involves the ability to effectively interact with all parties associated with a company.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS440XG653B0ZK3926JF"),
    },
    Alignment {
        name: "Good Governance",
        url: "https://lightcast.io/open-skills/skills/KS124DZ6MTMB30HLXWGJ/good-governance",
        description: Some(
            "Good governance is a crucial skill that involves the efficient, transparent, and accountable management of public resources for the benefit of citizens.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS124DZ6MTMB30HLXWGJ"),
    },
    Alignment {
        name: "Data collection",
        url: "https://lightcast.io/open-skills/skills/KS122NR6NY1KB0TD40G4/data-collection",
        description: Some(
            "Data collection is the process of gathering and measuring information on targeted variables using established methods, in order to answer specific research questions or test hypotheses.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122NR6NY1KB0TD40G4"),
    },
    Alignment {
        name: "Diversity and Inclusion",
        url: "https://lightcast.io/open-skills/skills/ES927EF31A5D2B127B50/diversity-and-inclusion",
        description: Some(
            "Diversity And Inclusion refers to the practice of creating an environment that values and respects individual differences, including but not limited to race, gender, age, sexual orientation, and abilities.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ES927EF31A5D2B127B50"),
    },
    Alignment {
        name: "Ethical leadership",
        url: "https://lightcast.io/open-skills/skills/KS123P2667PGVD3D6FLQ/ethical-leadership",
        description: Some(
            "Ethical Leadership involves efficiently overseeing resources and operations to promote long-term environmental, social, and economic viability within an organization.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS123P2667PGVD3D6FLQ"),
    },
    Alignment {
        name: "Universal design",
        url: "https://lightcast.io/open-skills/skills/KS1253C69TGYYVPJNSRT/universal-design",
        description: Some(
            "Universal Design is a design approach that prioritizes creating products, environments, and experiences that are accessible and usable by everyone, regardless of their age, ability, or disability.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS1253C69TGYYVPJNSRT"),
    },
    Alignment {
        name: "Community-Based Participatory Research",
        url: "https://lightcast.io/open-skills/skills/KS121G75X4VFH83Z1KKH/community-based-participatory-research",
        description: Some(
            "Community-Based Participatory Research (CBPR) is a collaborative research approach that involves community members, researchers, and other stakeholders in all aspects of the research process.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS121G75X4VFH83Z1KKH"),
    },
];

static ONE_MILLION_2024_ALIGNMENTS: [Alignment; 9] = [
    Alignment {
        name: "Data collection",
        url: "https://lightcast.io/open-skills/skills/KS122NR6NY1KB0TD40G4/data-collection",
        description: Some(
            "Data collection is the process of gathering and measuring information on targeted variables using established methods, in order to answer specific research questions or test hypotheses.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122NR6NY1KB0TD40G4"),
    },
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
        name: "Critical thinking",
        url: "https://lightcast.io/open-skills/skills/KS122J56PZMMFL1SVY1F/critical-thinking",
        description: Some(
            "Critical thinking is a skill that allows individuals to analyze and evaluate information to make sound judgements and decisions.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122J56PZMMFL1SVY1F"),
    },
    Alignment {
        name: "Digital literacy",
        url: "https://lightcast.io/open-skills/skills/ESB04BCB6189A2CB271E/digital-literacy",
        description: Some(
            "Digital Literacy is the ability to use, understand, and navigate digital technologies and engage with digital devices, applications, and information sources in a meaningful and productive way.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ESB04BCB6189A2CB271E"),
    },
    Alignment {
        name: "Collaboration",
        url: "https://lightcast.io/open-skills/skills/KS7AASDV6UOE7VFBGF4D/collaboration",
        description: Some(
            "Collaboration is a common skill that is essential for success in many fields. It involves working together with others to achieve a common goal, communicating effectively, and being open to new ideas and perspectives.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS7AASDV6UOE7VFBGF4D"),
    },
    Alignment {
        name: "Scientific Literacy",
        url: "https://lightcast.io/open-skills/skills/ESDECC52D71F603E3462/scientific-literacy",
        description: Some(
            "Scientific Literacy refers to the ability to understand, evaluate, and apply scientific information and concepts.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ESDECC52D71F603E3462"),
    },
    Alignment {
        name: "Problem solving",
        url: "https://lightcast.io/open-skills/skills/KS125F678LV2KB3Z5XW0/problem-solving",
        description: Some(
            "Problem solving is the process of identifying, analyzing and resolving problems that can arise in any situation.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS125F678LV2KB3Z5XW0"),
    },
    Alignment {
        name: "Communication",
        url: "https://lightcast.io/open-skills/skills/KS122556LMQ829GZCCRV/communication",
        description: Some(
            "Communication is the ability to effectively interact, convey information, and collaborate with others in a clear and understandable manner.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122556LMQ829GZCCRV"),
    },
    Alignment {
        name: "Detail-oriented",
        url: "https://lightcast.io/open-skills/skills/KSQPGV2XZO8UMNCC49LK/detail-oriented",
        description: Some(
            "Detail Oriented is a skill that involves a strong focus on precision and thoroughness when performing tasks or handling information.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KSQPGV2XZO8UMNCC49LK"),
    },
];

static AMBASSADOR_ALIGNMENTS: [Alignment; 6] = [
    Alignment {
        name: "Community outreach",
        url: "https://lightcast.io/open-skills/skills/BGS6F9F4F7DBABC95CE3/community-outreach",
        description: Some(
            "Community Outreach refers to the process of engaging with and providing support to local communities to address their needs and concerns.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("BGS6F9F4F7DBABC95CE3"),
    },
    Alignment {
        name: "Event planning",
        url: "https://lightcast.io/open-skills/skills/KS122CZ71K19596XHQ92/event-planning",
        description: Some(
            "Event Planning is a skill that involves the strategic organization and execution of events such as meetings, conferences, and parties.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122CZ71K19596XHQ92"),
    },
    Alignment {
        name: "Communication sciences",
        url: "https://lightcast.io/open-skills/skills/KS1225B6W0FJQHLLG6R7/communication-sciences",
        description: Some(
            "Communication Sciences is a field of study that focuses on the processes of communication, including language, speech, hearing, and interpersonal interactions.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS1225B6W0FJQHLLG6R7"),
    },
    Alignment {
        name: "Resource Management",
        url: "https://lightcast.io/open-skills/skills/KS4401D6PYRMFQC4HR62/resource-management",
        description: Some(
            "Resource management refers to the process of planning, organizing, and allocating resources in order to achieve specific goals in the most efficient and effective manner possible.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS4401D6PYRMFQC4HR62"),
    },
    Alignment {
        name: "Leadership",
        url: "https://lightcast.io/open-skills/skills/KS124JB619VXG6RQ810C/leadership",
        description: Some(
            "Leadership is a skill that is common among successful individuals in various fields. It involves the ability to motivate and guide a team towards achieving common goals.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS124JB619VXG6RQ810C"),
    },
    Alignment {
        name: "Curiosity",
        url: "https://lightcast.io/open-skills/skills/ESA420F05EBBD34B3C2A/curiosity",
        description: Some(
            "Curiosity refers to the inclination to explore, seek out new information, and ask questions.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ESA420F05EBBD34B3C2A"),
    },
];

static ONE_MILLION_2025_ALIGNMENTS: [Alignment; 9] = [
    Alignment {
        name: "Data collection",
        url: "https://lightcast.io/open-skills/skills/KS122NR6NY1KB0TD40G4/data-collection",
        description: Some(
            "Data collection is the process of gathering and measuring information on targeted variables using established methods, in order to answer specific research questions or test hypotheses.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122NR6NY1KB0TD40G4"),
    },
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
        name: "Critical thinking",
        url: "https://lightcast.io/open-skills/skills/KS122J56PZMMFL1SVY1F/critical-thinking",
        description: Some(
            "Critical thinking is a skill that allows individuals to analyze and evaluate information to make sound judgements and decisions.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122J56PZMMFL1SVY1F"),
    },
    Alignment {
        name: "Digital literacy",
        url: "https://lightcast.io/open-skills/skills/ESB04BCB6189A2CB271E/digital-literacy",
        description: Some(
            "Digital Literacy is the ability to use, understand, and navigate digital technologies and engage with digital devices, applications, and information sources in a meaningful and productive way.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ESB04BCB6189A2CB271E"),
    },
    Alignment {
        name: "Collaboration",
        url: "https://lightcast.io/open-skills/skills/KS7AASDV6UOE7VFBGF4D/collaboration",
        description: Some(
            "Collaboration is a common skill that is essential for success in many fields. It involves working together with others to achieve a common goal, communicating effectively, and being open to new ideas and perspectives.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS7AASDV6UOE7VFBGF4D"),
    },
    Alignment {
        name: "Scientific Literacy",
        url: "https://lightcast.io/open-skills/skills/ESDECC52D71F603E3462/scientific-literacy",
        description: Some(
            "Scientific Literacy refers to the ability to understand, evaluate, and apply scientific information and concepts.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ESDECC52D71F603E3462"),
    },
    Alignment {
        name: "Problem solving",
        url: "https://lightcast.io/open-skills/skills/KS125F678LV2KB3Z5XW0/problem-solving",
        description: Some(
            "Problem solving is the process of identifying, analyzing and resolving problems that can arise in any situation.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS125F678LV2KB3Z5XW0"),
    },
    Alignment {
        name: "Communication",
        url: "https://lightcast.io/open-skills/skills/KS122556LMQ829GZCCRV/communication",
        description: Some(
            "Communication is the ability to effectively interact, convey information, and collaborate with others in a clear and understandable manner.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122556LMQ829GZCCRV"),
    },
    Alignment {
        name: "Detail-oriented",
        url: "https://lightcast.io/open-skills/skills/KSQPGV2XZO8UMNCC49LK/detail-oriented",
        description: Some(
            "Detail Oriented is a skill that involves a strong focus on precision and thoroughness when performing tasks or handling information.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KSQPGV2XZO8UMNCC49LK"),
    },
];

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
