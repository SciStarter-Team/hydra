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
    CitizenScienceForK12Educators,
}

impl Badge {
    /// Every badge the site knows about.
    ///
    /// Written once so that a test covering "all badges" cannot quietly
    /// miss the next one added.
    pub const ALL: [Badge; 11] = [
        Badge::FoundationsOfCitizenScience,
        Badge::LibrariesAsHubsForCitizenScience,
        Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience,
        Badge::TeachingInHigherEducationWithCitizenScience,
        Badge::DataEthicsForPractitioners,
        Badge::IdealParticipatoryScience,
        Badge::OneMillionActsOfScience2024,
        Badge::SciStarterAmbassador,
        Badge::OneMillionActsOfScience2025,
        Badge::OneMillionActsOfScience2026,
        Badge::CitizenScienceForK12Educators,
    ];

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
            Badge::CitizenScienceForK12Educators => "01KPAY9A4VN929P02MECQ1W90N",
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
            Badge::OneMillionActsOfScience2026 => "2.50 Million Acts of Science 2026",
            Badge::CitizenScienceForK12Educators => "Citizen Science for K-12 Educators",
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
                    "/-static/img/badges/badge-acts-2026.png"
                )
            }
            Badge::CitizenScienceForK12Educators => {
                concat!(
                    "https://",
                    env!("DOMAIN"),
                    "/-static/img/badges/badge-k12.png"
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
            Badge::OneMillionActsOfScience2026 => "img/badges/badge-acts-2026.png",
            Badge::CitizenScienceForK12Educators => "img/badges/badge-k12.png",
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
            Badge::CitizenScienceForK12Educators => Some("https://scistarter.org/training"),
        }
    }

    /// Returns the badge class description
    pub fn description(&self) -> &'static str {
        match self {
            Badge::FoundationsOfCitizenScience => {
                "The Foundations of Citizen Science badge is awarded to citizen scientists who have translated their curiosity into impact through their demonstration of foundational citizen science skills and their contribution to advancing scientific research on the SciStarter platform."
            }
            Badge::LibrariesAsHubsForCitizenScience => {
                "The \"Libraries as Community Hubs for Citizen Science\" badge is awarded to librarians and library staff who have demonstrated a commitment to citizen science and the requisite skills necessary for advancing scientific research through community-centered library programming and services."
            }
            Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience => {
                "The \"Building Data Literacy Through Community and Citizen Science\" badge is awarded to citizen scientists who applied their skills in data literacy to citizen science projects and can use their skills to help others engage with citizen science projects."
            }
            Badge::TeachingInHigherEducationWithCitizenScience => {
                "The \"Teaching in Higher Education with Citizen Science\" badge is awarded to faculty within higher education institutions who have demonstrated proficiencies in incorporating and implementing citizen science programming into their professional practice."
            }
            Badge::DataEthicsForPractitioners => {
                "The \"Data Ethics for Practitioners\" badge is awarded to practitioners and leaders of participatory science projects, including citizen science and community science projects, who have demonstrated an ability to \"think like an ethicist.\""
            }
            Badge::IdealParticipatoryScience => {
                "The \"IDEAL Participatory Science\" badge is awarded to practitioners and leaders of large-scale participatory science projects who are able and willing to address the unique challenges of inclusion, diversity, equity, and accessibility. When large-scale projects adopt inclusive, equitable, and accessible practices, they can then support diversity and broaden participation."
            }
            Badge::OneMillionActsOfScience2024 => {
                "In the spirit of collective achievement and scientific exploration, the \"One Million Acts of Science\" badge honors dedicated volunteers who contribute to the collective goal of completing one million scientific acts during Citizen Science Month (April 2024) via the SciStarter platform. By engaging with and contributing to SciStarter’s Affiliate projects - and effectively linking these contributions - volunteers showcase their exceptional dedication to advancing scientific knowledge and harnessing the power of the community for substantial impact. This badge celebrates their crucial role in driving a global movement toward a deeper understanding of our world, acknowledging their contributions as pivotal to the fabric of citizen science."
            }
            Badge::SciStarterAmbassador => {
                "The SciStarter Ambassador badge is awarded to individuals who have completed the necessary training and demonstrated their commitment to promoting citizen science initiatives in their communities. As a SciStarter Ambassador, you will champion, connect, and facilitate citizen science activities, leveraging resources from SciStarter and local community organizations."
            }
            Badge::OneMillionActsOfScience2025 => {
                "In the spirit of collective achievement and scientific exploration, the \"One Million Acts of Science\" badge honors dedicated volunteers who contribute to the collective goal of completing one million scientific acts during Citizen Science Month (April 2025) via the SciStarter platform. By engaging with and contributing to SciStarter’s Affiliate projects - and effectively linking these contributions - volunteers showcase their exceptional dedication to advancing scientific knowledge and harnessing the power of the community for substantial impact. This badge celebrates their crucial role in driving a global movement toward a deeper understanding of our world, acknowledging their contributions as pivotal to the fabric of citizen science."
            }
            Badge::OneMillionActsOfScience2026 => {
                "The “2.50 Million Acts of Science” badge celebrates the people who made it happen: volunteers whose curiosity and commitment helped advance scientific knowledge worldwide during Citizen Science Month (April 2026). By contributing to SciStarter Affiliate projects or self-reporting Acts on CitizenScienceMonth.org, participants joined a global effort to reach 2.50 Million Acts of Science. This badge is awarded to participants with SciStarter accounts, recognizing their impact as part of a powerful, people-driven science movement."
            }
            Badge::CitizenScienceForK12Educators => {
                "The Citizen Science for K-12 Educators badge is awarded to individuals who have translated their curiosity into impact by demonstrating foundational citizen science skills and learning how to bring citizen science into their own K-12 classrooms."
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
            Badge::CitizenScienceForK12Educators => "https://scistarter.org/training",
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
            Badge::OneMillionActsOfScience2026 => include_str!("acts_2026.md"),
            Badge::CitizenScienceForK12Educators => include_str!("k12_educators.md"),
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
            Badge::CitizenScienceForK12Educators => &K12_EDUCATORS_ALIGNMENTS,
        }
    }

    /// Returns the tags associated with this badge
    pub fn tags(&self) -> &'static [&'static str] {
        match self {
            Badge::FoundationsOfCitizenScience => &[
                "citizen science",
                "global issues",
                "common good",
                "learning",
                "volunteering",
                "curiosity",
            ],
            Badge::LibrariesAsHubsForCitizenScience => &[
                "professional development",
                "library science",
                "library programming",
                "facilitation",
                "community engagement",
                "developing partnerships",
                "community outreach",
                "citizen science",
            ],
            Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience => &[
                "citizen science",
                "data literacy",
                "common good",
                "data types",
                "statistics",
                "data quality",
            ],
            Badge::TeachingInHigherEducationWithCitizenScience => &[
                "citizen science",
                "data literacy",
                "common good",
                "higher education",
            ],
            Badge::DataEthicsForPractitioners => &[
                "professional development",
                "community engagement",
                "citizen science",
            ],
            Badge::IdealParticipatoryScience => &[],
            Badge::OneMillionActsOfScience2024 => &[],
            Badge::SciStarterAmbassador => &[
                "citizen science",
                "scistarter",
                "science communication",
                "community outreach",
            ],
            Badge::OneMillionActsOfScience2025 => &[],
            Badge::OneMillionActsOfScience2026 => &[],
            Badge::CitizenScienceForK12Educators => &[
                "citizen science",
                "K-12 education",
                "professional development",
                "experiential learning",
                "curriculum design",
                "teaching",
            ],
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
            "01KPAY9A4VN929P02MECQ1W90N" => Some(Badge::CitizenScienceForK12Educators),
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
            "Data Analysis is the ability to inspect, cleanse, transform, and model data with the goal to discover useful information, inform conclusions, and support decision-making. This skill involves applying various analytical methods to interpret datasets, identify patterns, and extract meaningful insights. Professionals with this skill contribute to evidence-based decision-making by presenting clear and actionable findings derived from complex datasets.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS120GV6C72JMSZKMTD7"),
    },
    Alignment {
        name: "Data entry",
        url: "https://lightcast.io/open-skills/skills/KS122NY60HCYYDXHQGQN/data-entry",
        description: Some(
            "Data Entry refers to the process of inputting, updating, and managing data in various formats within computer systems or databases. This skill involves accurately transcribing information from physical or digital sources into electronic formats, ensuring data integrity and accessibility. Knowledge of Data Entry is utilized in various sectors to maintain organized records, facilitate data analysis, and support decision-making processes by providing reliable information.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122NY60HCYYDXHQGQN"),
    },
    Alignment {
        name: "Information gathering",
        url: "https://lightcast.io/open-skills/skills/ESE2D5EA264FB06F5C4F/information-gathering",
        description: Some(
            "Information gathering is the process of collecting and analyzing data or facts in order to gain insight, solve problems, or make decisions. It involves various methods such as research, interviews, surveys, observation, and data analysis. This skill is critical in many industries, including business, law, government, and journalism. Effective information gathering requires attention to detail, critical thinking, and the ability to analyze and interpret data.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ESE2D5EA264FB06F5C4F"),
    },
    Alignment {
        name: "Questioning skills",
        url: "https://lightcast.io/open-skills/skills/KSG21Y30BQVH1S8XMJHK/questioning-skills",
        description: Some(
            "Questioning skills refer to the ability to ask thoughtful and open-ended questions that lead to deeper understanding and insight. This skill is important in various contexts, ranging from education and business to personal relationships. Effective questioning involves active listening, curiosity, empathy, and an ability to think critically and creatively. Asking the right questions can help clarify goals, identify problems, generate ideas, and foster better communication and collaboration. It is a common skill that can be developed and improved with practice.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KSG21Y30BQVH1S8XMJHK"),
    },
];

static LIBRARIES_ALIGNMENTS: [Alignment; 12] = [
    Alignment {
        name: "Evidence-based practice",
        url: "https://lightcast.io/open-skills/skills/KS123987840L8J17862K/evidence-based-practice",
        description: Some(
            "Evidence-Based Practice refers to the systematic approach of integrating the best available research evidence with clinical expertise and patient values to inform decision-making in healthcare and other fields. This skill involves critically appraising and applying relevant research findings to improve outcomes and enhance the quality of care. Knowledge of Evidence-Based Practice is used to guide interventions, develop policies, and implement strategies that are effective and tailored to the needs of individuals or populations.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS123987840L8J17862K"),
    },
    Alignment {
        name: "Library And Information Services",
        url: "https://lightcast.io/open-skills/skills/KS125RC6CTWJ5S7841WG/library-and-information-services",
        description: Some(
            "Library and Information Services refer to the specialized skills and techniques required to manage, collect, organize, and disseminate information resources to support research, learning, and other objectives. This field involves proficiency in areas like information retrieval, classification and cataloging, user services, information technology, and information management. Libraries and information centers use these skills to provide resources, services, and support to diverse user groups, including students, researchers, professionals, and the general public. Overall, Library and Information Services are an essential aspect of facilitating access to organized and quality information materials that are critical to different facets of society.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS125RC6CTWJ5S7841WG"),
    },
    Alignment {
        name: "Library Services",
        url: "https://lightcast.io/open-skills/skills/KS125TG66T8TXK7B2VT3/library-services",
        description: Some(
            "Library services are specialized skills that involve managing, organizing, and preserving information resources, as well as providing access to those resources for individuals and organizations. These skills include cataloging, reference services, collection development, information literacy instruction, and preservation and conservation of materials. Library professionals use these skills to support research, learning, and intellectual pursuits of people in their communities.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS125TG66T8TXK7B2VT3"),
    },
    Alignment {
        name: "Communication",
        url: "https://lightcast.io/open-skills/skills/KS122556LMQ829GZCCRV/communication",
        description: Some(
            "Communication is the ability to effectively interact, convey information, and collaborate with others in a clear and understandable manner. It involves the skill of translating complex data and information into a format that is comprehensible to diverse audiences. This skill is crucial in professional, personal, and academic settings, enabling individuals to exchange ideas, solve problems, and work together harmoniously. Effective communication fosters understanding and cooperation, making it an essential component of successful relationships and organizations.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122556LMQ829GZCCRV"),
    },
    Alignment {
        name: "Information literacy",
        url: "https://lightcast.io/open-skills/skills/KS1254G6X37NS306Y596/information-literacy",
        description: Some(
            "Information literacy is a common skill that involves the ability to recognize, locate, evaluate, and effectively use information. It is an essential skill for individuals to be effective in today's digital age, as it allows them to make informed decisions, conduct research, and communicate with others using different media platforms. This skill requires critical thinking, problem-solving, and the ability to use technology to access and evaluate information effectively.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS1254G6X37NS306Y596"),
    },
    Alignment {
        name: "Interdisciplinary Teaching",
        url: "https://lightcast.io/open-skills/skills/KS125856JH9VGVNY0TBP/interdisciplinary-teaching",
        description: Some(
            "Interdisciplinary Teaching refers to an educational approach that integrates concepts and methods from multiple disciplines to enhance learning experiences. This skill involves designing and implementing curricula that connect different subject areas, fostering critical thinking and problem-solving abilities among students. Knowledge of Interdisciplinary Teaching is used to create collaborative learning environments where students can apply diverse perspectives to real-world issues, promoting a deeper understanding of complex topics.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS125856JH9VGVNY0TBP"),
    },
    Alignment {
        name: "Library Instruction",
        url: "https://lightcast.io/open-skills/skills/KS12100638C964SSQDJD/library-instruction",
        description: Some(
            "Library Instruction refers to the teaching of information literacy skills that enable individuals to effectively locate, evaluate, and use information resources. This skill encompasses the development of instructional materials and the delivery of training sessions that guide users in navigating library systems and utilizing various research tools. Knowledge of Library Instruction is used to enhance users' research capabilities, promote critical thinking, and foster lifelong learning in academic and public library settings.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS12100638C964SSQDJD"),
    },
    Alignment {
        name: "Lifelong Learning",
        url: "https://lightcast.io/open-skills/skills/KS125TX75XTZZXVJL8B5/lifelong-learning",
        description: Some(
            "Lifelong learning is the \"ongoing, voluntary, and self-motivated\" pursuit of knowledge for either personal or professional reasons. It is important for an individual's competitiveness and employability, but also enhances social inclusion, active citizenship, and personal development.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS125TX75XTZZXVJL8B5"),
    },
    Alignment {
        name: "Teaching",
        url: "https://lightcast.io/open-skills/skills/KS120PQ6XD1JWMFQVJNW/teaching",
        description: Some(
            "Teaching is a common skill that involves the transfer of knowledge, skills, and values from one person to another. It involves designing and delivering effective learning experiences that engage students and promote understanding. Effective teaching requires the ability to communicate clearly, listen actively, adapt to different learning styles, and provide constructive feedback. It is an important skill that is required across a wide range of professions and industries.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS120PQ6XD1JWMFQVJNW"),
    },
    Alignment {
        name: "Training and development",
        url: "https://lightcast.io/open-skills/skills/KS122F96DJHT3ZXB8FCZ/training-and-development",
        description: Some(
            "Training And Development involves designing and implementing educational programs to enhance employees' skills and knowledge. This skill encompasses assessing training needs, creating instructional materials, and evaluating the effectiveness of training initiatives. It aims to improve individual and organizational performance by fostering continuous learning and professional growth.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122F96DJHT3ZXB8FCZ"),
    },
    Alignment {
        name: "ALA’s Core Competencies of Librarianship",
        url: "https://www.ala.org/sites/default/files/educationcareers/content/2022%20ALA%20Core%20Competences%20of%20Librarianship_FINAL.pdf",
        description: Some(
            "## ALA’s Core Competencies of Librarianship\nLibraries and the communities they serve are continually evolving. As such, continuing education, professional growth, and a commitment to lifelong learning are key components of a well-informed library professional. Library professionals, regardless of their specific title and/or role, should have the foundational skills and understanding to:\n\n- 3A. Participate in and lead on-going professional development to better serve their communities.\n- 3B. Recognize the role of the library in continuing education and lifelong learning initiatives.\n- 3C. Employ multiple techniques to accommodate diverse learning preferences to promote lifelong learning.\n- 3D. Understand established and new learning theories, principles of critical and inclusive pedagogy, instructional methods, and learning outcomes assessment; and apply them to educational initiatives in information settings.",
        ),
        framework: None,
        code: None,
    },
    Alignment {
        name: "Professional Competencies for Health Sciences Librarians",
        url: "https://www.mlanet.org/education/policy/",
        description: Some(
            "(3) Understand the principles and practices related to providing information services to meet users' needs, including:\n- The information needs of health practitioners, researchers, administrators, educators, students, patients, consumers, and the general public\n- The institution's information policies\n- Methods of information delivery and access including consideration of the specific information needs of diverse populations\n- Information services management",
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
            "Data Literacy is the ability to read, understand, analyze and interpret data. It involves specialized skills such as data collection, analysis, communication and decision-making. Data literacy helps individuals make informed decisions based on data insights and makes them better equipped to navigate in a data-driven world.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ES476D9219E938F5CB96"),
    },
    Alignment {
        name: "Data-Driven Decision Making",
        url: "https://lightcast.io/open-skills/skills/ESF14A354DDD69B5FBD4/data-driven-decision-making",
        description: Some(
            "Data-Driven Decision Making is the process of utilizing relevant and reliable data to inform and guide decision-making within an organization. This skill involves the ability to collect, analyze, and interpret data to make informed choices that align with organizational goals and objectives. By employing statistical and analytical techniques, individuals with this skill can optimize decision outcomes, enhance efficiency, and achieve better results across various domains, ranging from business strategy to operational processes.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ESF14A354DDD69B5FBD4"),
    },
    Alignment {
        name: "Critical thinking",
        url: "https://lightcast.io/open-skills/skills/KS122J56PZMMFL1SVY1F/critical-thinking",
        description: Some(
            "Critical thinking is a skill that allows individuals to analyze and evaluate information to make sound judgements and decisions. It involves the ability to question assumptions, identify biases, and examine evidence to reach logical conclusions. It is a common skill that is essential in various areas of life, including education, career, and personal relationships.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122J56PZMMFL1SVY1F"),
    },
    Alignment {
        name: "Data analysis",
        url: "https://lightcast.io/open-skills/skills/KS120GV6C72JMSZKMTD7/data-analysis",
        description: Some(
            "Data Analysis is the ability to inspect, cleanse, transform, and model data with the goal to discover useful information, inform conclusions, and support decision-making. This skill involves applying various analytical methods to interpret datasets, identify patterns, and extract meaningful insights. Professionals with this skill contribute to evidence-based decision-making by presenting clear and actionable findings derived from complex datasets.",
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
            "Constructivism refers to a learning theory that emphasizes the role of learners in actively constructing their own understanding and knowledge through experiences and reflection. This skill involves applying principles of experiential learning, critical thinking, and collaboration to facilitate deeper comprehension and retention of information. Knowledge of Constructivism is used to design educational environments and curricula that promote inquiry-based learning, allowing learners to engage with content meaningfully and develop problem-solving abilities.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KSLWYCD49AGA2P0SY8UE"),
    },
    Alignment {
        name: "Experiential Learning",
        url: "https://lightcast.io/open-skills/skills/KS123R966Z5R6D81XZK4/experiential-learning",
        description: Some(
            "Experiential learning is a unique educational approach that emphasizes hands-on, practical experience as a means of acquiring knowledge and skills. It involves actively engaging in the learning process through various activities that challenge and stimulate learners, such as internships, field trips, simulations, and collaborative projects. This approach is becoming increasingly popular in educational settings, as it allows learners to apply theoretical concepts in real-world situations, and acquire valuable skills that can be applied in practice. However, designing effective experiential learning programs requires specialized skills and expertise, including creating clear goals and objectives, designing appropriate activities and assessments, and providing supportive feedback and guidance.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS123R966Z5R6D81XZK4"),
    },
    Alignment {
        name: "Experiential Education",
        url: "https://lightcast.io/open-skills/skills/KS123R9698P8RCDZJ7H8/experiential-education",
        description: Some(
            "Experiential education is a specialized approach to learning that emphasizes hands-on, real-world experiences as a primary means of teaching. It involves the use of activities, projects, and other experiential exercises to help learners develop practical skills and knowledge in a particular subject. This approach requires specialized skills such as facilitation, curriculum design, assessment, and reflection to create meaningful and engaging learning experiences.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS123R9698P8RCDZJ7H8"),
    },
    Alignment {
        name: "Formative And Summative Assessments",
        url: "https://lightcast.io/open-skills/skills/ESAEA6B36CACC973C466/formative-and-summative-assessments",
        description: Some(
            "Formative and summative assessments are crucial components of the learning process that help educators gauge the effectiveness of their teaching practices and track their students' progress. Formative assessments are used throughout the learning cycle to monitor student understanding, provide feedback, and adjust instruction. Summative assessments are used at the end of a unit or course to evaluate the overall learning outcomes and measure student achievement against established standards. Successfully implementing formative and summative assessments requires specialized skills, including the ability to design effective assessments, analyze data, and use the results to improve teaching practices and student outcomes.",
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
            "Data Governance is the ability to assess, manage, use, improve, monitor, maintain, and protect organizational information through a system of decision rights and accountabilities for information-related processes. This skill ensures the establishment of policies and procedures for effective data management, quality control, and compliance. Users proficient in Data Governance contribute to the establishment of a structured framework that governs data usage, safeguards data integrity, and aligns with organizational goals and regulatory requirements.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122P378DGNVX4NKQKN"),
    },
    Alignment {
        name: "Data integrity",
        url: "https://lightcast.io/open-skills/skills/KS122P86NZFH1GP38G15/data-integrity",
        description: Some(
            "Data Integrity is the capability to maintain the accuracy, consistency, and reliability of data throughout its lifecycle, ensuring that the data remains free from errors and inconsistencies. This skill involves implementing measures and processes to validate and safeguard data quality, contributing to the overall trustworthiness of information within an organization. Professionals skilled in Data Integrity play a crucial role in upholding the quality standards of data, supporting informed decision-making and promoting the reliability of organizational information.",
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
            "Diversity awareness is an important skill to have in today's global and diverse society. It involves recognizing and respecting differences among people, including differences in race, ethnicity, gender, sexual orientation, religion, and culture. Being aware of these differences can help individuals develop a more inclusive and tolerant attitude towards others, and help to create a more accepting and respectful society. It is an essential skill for building relationships and effective communication with people from different backgrounds.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS123285XWQS1TVY0YJ4"),
    },
    Alignment {
        name: "Leadership",
        url: "https://lightcast.io/open-skills/skills/KS124JB619VXG6RQ810C/leadership",
        description: Some(
            "Leadership is a skill that is common among successful individuals in various fields. It involves the ability to motivate and guide a team towards achieving common goals. Good leadership qualities include effective communication, empathy, decision-making skills, adaptability, and vision. Leadership can be developed and honed through training, practice, and learning from others.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS124JB619VXG6RQ810C"),
    },
    Alignment {
        name: "Self-awareness",
        url: "https://lightcast.io/open-skills/skills/ES88C410DA4EE6A2FE77/self-awareness",
        description: Some(
            "Self-awareness is the ability to understand and be mindful of your own thoughts, emotions, and behaviors. It is a common skill that helps individuals identify and manage their strengths and weaknesses, and make more informed decisions about their lives. Developing self-awareness requires regular self-reflection, introspection, and feedback from others.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ES88C410DA4EE6A2FE77"),
    },
    Alignment {
        name: "Self-Directed Learning",
        url: "https://lightcast.io/open-skills/skills/ES2F89D25BA6FFC32FA9/self-directed-learning",
        description: Some(
            "Self-Directed Learning refers to the process in which individuals take the initiative to identify their learning needs, set goals, and seek out resources to acquire knowledge and skills independently. This skill involves self-motivation, critical thinking, and the ability to evaluate one's progress and adapt learning strategies accordingly. Self-Directed Learning is used to enhance personal and professional development by enabling individuals to pursue topics of interest, fill knowledge gaps, and stay current in their fields. It fosters lifelong learning and adaptability in an ever-changing environment.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ES2F89D25BA6FFC32FA9"),
    },
    Alignment {
        name: "Decision making",
        url: "https://lightcast.io/open-skills/skills/KS121W46CJXQ5ZQ506MS/decision-making",
        description: Some(
            "Decision making is the process of choosing between two or more options to reach a desired outcome. It is a common skill that is used in all aspects of life, from personal choices to professional decisions. Effective decision making requires analyzing and evaluating available information, considering the potential outcomes, and weighing the pros and cons of each option. The ability to make sound decisions is an important skill to develop for success in work, relationships, and personal growth.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS121W46CJXQ5ZQ506MS"),
    },
    Alignment {
        name: "Intercultural Competence",
        url: "https://lightcast.io/open-skills/skills/KS1258479NCTHZ20PFDH/intercultural-competence",
        description: Some(
            "Intercultural competence is the ability to communicate effectively and appropriately with people from different cultures. It involves understanding cultural differences and adapting one's behavior and communication style accordingly to facilitate understanding and collaboration. It is becoming an increasingly important skill in today's globalized world, where people from different cultures are increasingly interacting in both personal and professional contexts.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS1258479NCTHZ20PFDH"),
    },
    Alignment {
        name: "Stakeholder engagement",
        url: "https://lightcast.io/open-skills/skills/KS440XG653B0ZK3926JF/stakeholder-engagement",
        description: Some(
            "Stakeholder Engagement is a critical skill that involves the ability to effectively interact with all parties associated with a company. It requires understanding the interests, needs, and levels of engagement of these stakeholders, and how the company's plans and actions may impact their objectives. This skill is used to foster strong relationships, facilitate communication, and align company strategies with stakeholder expectations, ultimately contributing to the successful achievement of organizational goals.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS440XG653B0ZK3926JF"),
    },
    Alignment {
        name: "Good Governance",
        url: "https://lightcast.io/open-skills/skills/KS124DZ6MTMB30HLXWGJ/good-governance",
        description: Some(
            "Good governance is a crucial skill that involves the efficient, transparent, and accountable management of public resources for the benefit of citizens. This skill requires specialized knowledge and experience in areas such as institutional design, public policy development, stakeholder engagement, and risk management, among others. Effective governance is essential for achieving sustainable economic growth, reducing poverty, promoting social welfare, and ensuring political stability. It involves the creation of institutions and mechanisms that can foster transparency, accountability, and public participation in decision-making processes. Good governance requires continuous learning, adaptability, and the ability to anticipate and respond to emerging challenges and changing circumstances.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS124DZ6MTMB30HLXWGJ"),
    },
    Alignment {
        name: "Data collection",
        url: "https://lightcast.io/open-skills/skills/KS122NR6NY1KB0TD40G4/data-collection",
        description: Some(
            "Data collection is the process of gathering and measuring information on targeted variables using established methods, in order to answer specific research questions or test hypotheses. This involves the use of specialized skills and techniques, including designing surveys, conducting interviews, using sensors and other instruments to collect physical data, and utilizing computer programs to manage and analyze data. Effective data collection is crucial for ensuring high-quality research and informing evidence-based decision-making in a variety of fields.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122NR6NY1KB0TD40G4"),
    },
    Alignment {
        name: "Diversity and Inclusion",
        url: "https://lightcast.io/open-skills/skills/ES927EF31A5D2B127B50/diversity-and-inclusion",
        description: Some(
            "Diversity And Inclusion refers to the practice of creating an environment that values and respects individual differences, including but not limited to race, gender, age, sexual orientation, and abilities. This skill encompasses strategies and policies that promote equitable treatment and opportunities for all individuals, fostering a culture of belonging. Knowledge of Diversity And Inclusion is used to enhance team collaboration, drive innovation, and improve organizational performance by leveraging diverse perspectives and experiences.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ES927EF31A5D2B127B50"),
    },
    Alignment {
        name: "Ethical leadership",
        url: "https://lightcast.io/open-skills/skills/KS123P2667PGVD3D6FLQ/ethical-leadership",
        description: Some(
            "Ethical Leadership involves efficiently overseeing resources and operations to promote long-term environmental, social, and economic viability within an organization. This skill is utilized to ensure that leadership decisions align with ethical and sustainable practices, fostering the well-being of both the organization and the broader community. Ethical leaders prioritize values, transparency, and responsible decision-making to achieve a positive and lasting impact on their organization and the world.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS123P2667PGVD3D6FLQ"),
    },
    Alignment {
        name: "Universal design",
        url: "https://lightcast.io/open-skills/skills/KS1253C69TGYYVPJNSRT/universal-design",
        description: Some(
            "Universal Design is a design approach that prioritizes creating products, environments, and experiences that are accessible and usable by everyone, regardless of their age, ability, or disability. It involves the careful consideration of physical, sensory, and cognitive factors to ensure that the design is inclusive and meets the needs of all users. Universal Design requires specialized skills in research, design, engineering, and testing, and involves collaboration and communication with diverse groups of people.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS1253C69TGYYVPJNSRT"),
    },
    Alignment {
        name: "Community-Based Participatory Research",
        url: "https://lightcast.io/open-skills/skills/KS121G75X4VFH83Z1KKH/community-based-participatory-research",
        description: Some(
            "Community-Based Participatory Research (CBPR) is a collaborative research approach that involves community members, researchers, and other stakeholders in all aspects of the research process. It requires specific skills and methodologies to engage the community, build trust, and ensure that research findings are meaningful, relevant, and useful to the community. CBPR can help address health disparities and other social issues, and it requires long-term commitment and investment.",
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
            "Data collection is the process of gathering and measuring information on targeted variables using established methods, in order to answer specific research questions or test hypotheses. This involves the use of specialized skills and techniques, including designing surveys, conducting interviews, using sensors and other instruments to collect physical data, and utilizing computer programs to manage and analyze data. Effective data collection is crucial for ensuring high-quality research and informing evidence-based decision-making in a variety of fields.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122NR6NY1KB0TD40G4"),
    },
    Alignment {
        name: "Data analysis",
        url: "https://lightcast.io/open-skills/skills/KS120GV6C72JMSZKMTD7/data-analysis",
        description: Some(
            "Data Analysis is the ability to inspect, cleanse, transform, and model data with the goal to discover useful information, inform conclusions, and support decision-making. This skill involves applying various analytical methods to interpret datasets, identify patterns, and extract meaningful insights. Professionals with this skill contribute to evidence-based decision-making by presenting clear and actionable findings derived from complex datasets.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS120GV6C72JMSZKMTD7"),
    },
    Alignment {
        name: "Critical thinking",
        url: "https://lightcast.io/open-skills/skills/KS122J56PZMMFL1SVY1F/critical-thinking",
        description: Some(
            "Critical thinking is a skill that allows individuals to analyze and evaluate information to make sound judgements and decisions. It involves the ability to question assumptions, identify biases, and examine evidence to reach logical conclusions. It is a common skill that is essential in various areas of life, including education, career, and personal relationships.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122J56PZMMFL1SVY1F"),
    },
    Alignment {
        name: "Digital literacy",
        url: "https://lightcast.io/open-skills/skills/ESB04BCB6189A2CB271E/digital-literacy",
        description: Some(
            "Digital Literacy is the ability to use, understand, and navigate digital technologies and engage with digital devices, applications, and information sources in a meaningful and productive way. This skill empowers individuals to effectively utilize digital tools, access online information, and communicate through digital platforms. It is essential for participating in the modern digital world, enhancing educational and professional opportunities, and staying informed and connected in today's technology-driven society.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ESB04BCB6189A2CB271E"),
    },
    Alignment {
        name: "Collaboration",
        url: "https://lightcast.io/open-skills/skills/KS7AASDV6UOE7VFBGF4D/collaboration",
        description: Some(
            "Collaboration is a common skill that is essential for success in many fields. It involves working together with others to achieve a common goal, communicating effectively, and being open to new ideas and perspectives. Collaboration skills can be learned and improved through practice, and are highly valued by employers and organizations. Collaboration can be achieved through various tools and methods, such as virtual collaboration software, team building exercises, and group brainstorming sessions.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS7AASDV6UOE7VFBGF4D"),
    },
    Alignment {
        name: "Scientific Literacy",
        url: "https://lightcast.io/open-skills/skills/ESDECC52D71F603E3462/scientific-literacy",
        description: Some(
            "Scientific Literacy refers to the ability to understand, evaluate, and apply scientific information and concepts. This skill encompasses knowledge of scientific principles, methodologies, and the ability to critically assess scientific claims and data. Scientific Literacy is used to make informed decisions about health, environment, and technology by enabling individuals to engage with scientific issues and understand their implications in everyday life.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ESDECC52D71F603E3462"),
    },
    Alignment {
        name: "Problem solving",
        url: "https://lightcast.io/open-skills/skills/KS125F678LV2KB3Z5XW0/problem-solving",
        description: Some(
            "Problem solving is the process of identifying, analyzing and resolving problems that can arise in any situation. It is a common skill that is required in many different industries and situations, and involves identifying the root cause of a problem, generating possible solutions, evaluating those solutions and implementing the best one. Successful problem solving involves critical thinking, creativity and communication skills. Learning how to effectively solve problems can help individuals and organizations to more efficiently achieve their goals and objectives.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS125F678LV2KB3Z5XW0"),
    },
    Alignment {
        name: "Communication",
        url: "https://lightcast.io/open-skills/skills/KS122556LMQ829GZCCRV/communication",
        description: Some(
            "Communication is the ability to effectively interact, convey information, and collaborate with others in a clear and understandable manner. It involves the skill of translating complex data and information into a format that is comprehensible to diverse audiences. This skill is crucial in professional, personal, and academic settings, enabling individuals to exchange ideas, solve problems, and work together harmoniously. Effective communication fosters understanding and cooperation, making it an essential component of successful relationships and organizations.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122556LMQ829GZCCRV"),
    },
    Alignment {
        name: "Detail-oriented",
        url: "https://lightcast.io/open-skills/skills/KSQPGV2XZO8UMNCC49LK/detail-oriented",
        description: Some(
            "Detail Oriented is a skill that involves a strong focus on precision and thoroughness when performing tasks or handling information. This skill is used to spot errors, maintain accuracy, and ensure quality in work. Detail-oriented individuals excel in tasks such as proofreading documents, conducting thorough research, performing quality control checks, and meticulously reviewing data to prevent mistakes and produce high-quality results.",
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
            "Community Outreach refers to the process of engaging with and providing support to local communities to address their needs and concerns. This skill involves identifying community resources, building relationships with stakeholders, and facilitating communication between organizations and community members. Knowledge of Community Outreach is used to develop programs and initiatives that promote social welfare, enhance community involvement, and foster collaboration among diverse groups. Effective Community Outreach contributes to the empowerment of individuals and the strengthening of community ties.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("BGS6F9F4F7DBABC95CE3"),
    },
    Alignment {
        name: "Event planning",
        url: "https://lightcast.io/open-skills/skills/KS122CZ71K19596XHQ92/event-planning",
        description: Some(
            "Event Planning is a skill that involves the strategic organization and execution of events such as meetings, conferences, and parties. It requires the ability to manage time effectively, consider safety measures, adhere to budget limitations, and meet specific business objectives. This skill is used to ensure that events are successful and meet the desired goals, while also ensuring a positive experience for all attendees.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122CZ71K19596XHQ92"),
    },
    Alignment {
        name: "Communication sciences",
        url: "https://lightcast.io/open-skills/skills/KS1225B6W0FJQHLLG6R7/communication-sciences",
        description: Some(
            "Communication Sciences is a field of study that focuses on the processes of communication, including language, speech, hearing, and interpersonal interactions. It involves a specialized set of skills, including critical thinking, problem-solving, and effective communication, and can be applied to a range of industries, including healthcare, education, business, and technology. Professionals in this field may work as speech therapists, researchers, educators, consultants, or communication specialists.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS1225B6W0FJQHLLG6R7"),
    },
    Alignment {
        name: "Resource Management",
        url: "https://lightcast.io/open-skills/skills/KS4401D6PYRMFQC4HR62/resource-management",
        description: Some(
            "Resource management refers to the process of planning, organizing, and allocating resources in order to achieve specific goals in the most efficient and effective manner possible. This can involve managing things like personnel, equipment, budgets, and time. Resource management requires specialized skills in order to ensure that resources are utilized in a way that maximizes their value and contributes to the overall success of the project or organization. Effective resource management can help to increase productivity, reduce costs, and improve outcomes.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS4401D6PYRMFQC4HR62"),
    },
    Alignment {
        name: "Leadership",
        url: "https://lightcast.io/open-skills/skills/KS124JB619VXG6RQ810C/leadership",
        description: Some(
            "Leadership is a skill that is common among successful individuals in various fields. It involves the ability to motivate and guide a team towards achieving common goals. Good leadership qualities include effective communication, empathy, decision-making skills, adaptability, and vision. Leadership can be developed and honed through training, practice, and learning from others.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS124JB619VXG6RQ810C"),
    },
    Alignment {
        name: "Curiosity",
        url: "https://lightcast.io/open-skills/skills/ESA420F05EBBD34B3C2A/curiosity",
        description: Some(
            "Curiosity refers to the inclination to explore, seek out new information, and ask questions. It drives individuals to investigate unfamiliar topics, acquire knowledge, and understand various perspectives. This skill is employed to enhance learning, problem-solving, and innovation by encouraging active engagement and exploration of new ideas or processes. Curiosity fosters a deeper understanding and continuous improvement in diverse fields and situations.",
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
            "Data collection is the process of gathering and measuring information on targeted variables using established methods, in order to answer specific research questions or test hypotheses. This involves the use of specialized skills and techniques, including designing surveys, conducting interviews, using sensors and other instruments to collect physical data, and utilizing computer programs to manage and analyze data. Effective data collection is crucial for ensuring high-quality research and informing evidence-based decision-making in a variety of fields.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122NR6NY1KB0TD40G4"),
    },
    Alignment {
        name: "Data analysis",
        url: "https://lightcast.io/open-skills/skills/KS120GV6C72JMSZKMTD7/data-analysis",
        description: Some(
            "Data Analysis is the ability to inspect, cleanse, transform, and model data with the goal to discover useful information, inform conclusions, and support decision-making. This skill involves applying various analytical methods to interpret datasets, identify patterns, and extract meaningful insights. Professionals with this skill contribute to evidence-based decision-making by presenting clear and actionable findings derived from complex datasets.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS120GV6C72JMSZKMTD7"),
    },
    Alignment {
        name: "Critical thinking",
        url: "https://lightcast.io/open-skills/skills/KS122J56PZMMFL1SVY1F/critical-thinking",
        description: Some(
            "Critical thinking is a skill that allows individuals to analyze and evaluate information to make sound judgements and decisions. It involves the ability to question assumptions, identify biases, and examine evidence to reach logical conclusions. It is a common skill that is essential in various areas of life, including education, career, and personal relationships.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122J56PZMMFL1SVY1F"),
    },
    Alignment {
        name: "Digital literacy",
        url: "https://lightcast.io/open-skills/skills/ESB04BCB6189A2CB271E/digital-literacy",
        description: Some(
            "Digital Literacy is the ability to use, understand, and navigate digital technologies and engage with digital devices, applications, and information sources in a meaningful and productive way. This skill empowers individuals to effectively utilize digital tools, access online information, and communicate through digital platforms. It is essential for participating in the modern digital world, enhancing educational and professional opportunities, and staying informed and connected in today's technology-driven society.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ESB04BCB6189A2CB271E"),
    },
    Alignment {
        name: "Collaboration",
        url: "https://lightcast.io/open-skills/skills/KS7AASDV6UOE7VFBGF4D/collaboration",
        description: Some(
            "Collaboration is a common skill that is essential for success in many fields. It involves working together with others to achieve a common goal, communicating effectively, and being open to new ideas and perspectives. Collaboration skills can be learned and improved through practice, and are highly valued by employers and organizations. Collaboration can be achieved through various tools and methods, such as virtual collaboration software, team building exercises, and group brainstorming sessions.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS7AASDV6UOE7VFBGF4D"),
    },
    Alignment {
        name: "Scientific Literacy",
        url: "https://lightcast.io/open-skills/skills/ESDECC52D71F603E3462/scientific-literacy",
        description: Some(
            "Scientific Literacy refers to the ability to understand, evaluate, and apply scientific information and concepts. This skill encompasses knowledge of scientific principles, methodologies, and the ability to critically assess scientific claims and data. Scientific Literacy is used to make informed decisions about health, environment, and technology by enabling individuals to engage with scientific issues and understand their implications in everyday life.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("ESDECC52D71F603E3462"),
    },
    Alignment {
        name: "Problem solving",
        url: "https://lightcast.io/open-skills/skills/KS125F678LV2KB3Z5XW0/problem-solving",
        description: Some(
            "Problem solving is the process of identifying, analyzing and resolving problems that can arise in any situation. It is a common skill that is required in many different industries and situations, and involves identifying the root cause of a problem, generating possible solutions, evaluating those solutions and implementing the best one. Successful problem solving involves critical thinking, creativity and communication skills. Learning how to effectively solve problems can help individuals and organizations to more efficiently achieve their goals and objectives.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS125F678LV2KB3Z5XW0"),
    },
    Alignment {
        name: "Communication",
        url: "https://lightcast.io/open-skills/skills/KS122556LMQ829GZCCRV/communication",
        description: Some(
            "Communication is the ability to effectively interact, convey information, and collaborate with others in a clear and understandable manner. It involves the skill of translating complex data and information into a format that is comprehensible to diverse audiences. This skill is crucial in professional, personal, and academic settings, enabling individuals to exchange ideas, solve problems, and work together harmoniously. Effective communication fosters understanding and cooperation, making it an essential component of successful relationships and organizations.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KS122556LMQ829GZCCRV"),
    },
    Alignment {
        name: "Detail-oriented",
        url: "https://lightcast.io/open-skills/skills/KSQPGV2XZO8UMNCC49LK/detail-oriented",
        description: Some(
            "Detail Oriented is a skill that involves a strong focus on precision and thoroughness when performing tasks or handling information. This skill is used to spot errors, maintain accuracy, and ensure quality in work. Detail-oriented individuals excel in tasks such as proofreading documents, conducting thorough research, performing quality control checks, and meticulously reviewing data to prevent mistakes and produce high-quality results.",
        ),
        framework: Some("LIGHTCAST"),
        code: Some("KSQPGV2XZO8UMNCC49LK"),
    },
];

static K12_EDUCATORS_ALIGNMENTS: [Alignment; 8] = [
    Alignment {
        name: "Instructional Design",
        url: "https://lightcast.io/open-skills/skills/KS1256C6DBYG39V53J4V/instructional-design",
        description: None,
        framework: Some("LIGHTCAST"),
        code: Some("KS1256C6DBYG39V53J4V"),
    },
    Alignment {
        name: "Curriculum Development",
        url: "https://lightcast.io/open-skills/skills/KS122LF6587VF0NZFZJW/curriculum-development",
        description: None,
        framework: Some("LIGHTCAST"),
        code: Some("KS122LF6587VF0NZFZJW"),
    },
    Alignment {
        name: "Curriculum Mapping",
        url: "https://lightcast.io/open-skills/skills/KS122LF709JLK7D9PL8J/curriculum-mapping",
        description: None,
        framework: Some("LIGHTCAST"),
        code: Some("KS122LF709JLK7D9PL8J"),
    },
    Alignment {
        name: "Teaching",
        url: "https://lightcast.io/open-skills/skills/KS120PQ6XD1JWMFQVJNW/teaching",
        description: None,
        framework: Some("LIGHTCAST"),
        code: Some("KS120PQ6XD1JWMFQVJNW"),
    },
    Alignment {
        name: "Lesson Planning",
        url: "https://lightcast.io/open-skills/skills/KS125SS6Z01PYP99SN5G/lesson-planning",
        description: None,
        framework: Some("LIGHTCAST"),
        code: Some("KS125SS6Z01PYP99SN5G"),
    },
    Alignment {
        name: "K-12 Education",
        url: "https://lightcast.io/open-skills/skills/BGS4866E4D4C7E1F272F/k-12-education",
        description: None,
        framework: Some("LIGHTCAST"),
        code: Some("BGS4866E4D4C7E1F272F"),
    },
    Alignment {
        name: "Interdisciplinary Teaching",
        url: "https://lightcast.io/open-skills/skills/KS125856JH9VGVNY0TBP/interdisciplinary-teaching",
        description: None,
        framework: Some("LIGHTCAST"),
        code: Some("KS125856JH9VGVNY0TBP"),
    },
    Alignment {
        name: "Experiential Learning",
        url: "https://lightcast.io/open-skills/skills/KS123R966Z5R6D81XZK4/experiential-learning",
        description: None,
        framework: Some("LIGHTCAST"),
        code: Some("KS123R966Z5R6D81XZK4"),
    },
];

static ALL_BADGES: [Badge; 11] = [
    Badge::FoundationsOfCitizenScience,
    Badge::LibrariesAsHubsForCitizenScience,
    Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience,
    Badge::TeachingInHigherEducationWithCitizenScience,
    Badge::DataEthicsForPractitioners,
    Badge::IdealParticipatoryScience,
    Badge::OneMillionActsOfScience2024,
    Badge::SciStarterAmbassador,
    Badge::OneMillionActsOfScience2025,
    Badge::OneMillionActsOfScience2026,
    Badge::CitizenScienceForK12Educators,
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

    /// The badge text orrery has to agree with, one digest per badge.
    ///
    /// Sorted by badge class. Update these **and orrery's copy of them** in
    /// the same change, never one alone: the point of the constant is that
    /// editing badge text in one repository fails a test in that repository
    /// and reminds whoever did it that the other repository says the same
    /// thing to the same earners.
    const BADGE_TEXT_DIGESTS: &[(&str, &str)] = &[
        (
            "01KEYVHY8RXNGXVACTR9TY02AD",
            "41706041eb91b6d9156cd14abd87fa0021f0e858bf3d62093adbbcbd4f473f89",
        ),
        (
            "01KPAY9A4VN929P02MECQ1W90N",
            "53f6e880268400f7c8fe20ebed7456eb1bdbcb95470eb83b2a52672b55c56736",
        ),
        (
            "1OeTfgY1Q-exWqz---kPAQ",
            "969f59cc2135e1445be1802fdaf31c864292cee3a94df7cd55a0a6551f7f038a",
        ),
        (
            "6Jg_IwmfQTaJ2kUNePsO5Q",
            "4cf8541805ebc6e01e57a2090d0eac717959b61c926206bd483b6d27034a993c",
        ),
        (
            "9csPkdhzQLSZuk-OWREaKg",
            "15c2e2bbfe212a8e1ffdaee537e5d861c585cba2216af696a3a064cad90872cc",
        ),
        (
            "CWZrsNMPSZSHqKJoz8pLMQ",
            "12b7a2695cfa15d4ee8ea8469e0a4e31217000509c0b4195fb7fd5d36f80a886",
        ),
        (
            "OP_wrFm7QauZgaXrZY-DhA",
            "271d381897c3443fb123fdfcea89502d9281ba52a959e4e93dc8e7b91c4a2cee",
        ),
        (
            "QLLs6Nm9TfqIj3_722yFQg",
            "8ba3a8ed13cd917ac957b5262f5bc8dd673cc8b9e3c4c9673be3195d1bbf9951",
        ),
        (
            "U431wPFaTaKwUQO0WhQJRg",
            "6009fe2f60b97028a72ebf6c5700637420aece9206bea67ff5ed7c88b8d127b9",
        ),
        (
            "a8cFJfeBTRec05glGM3_9w",
            "6170c4ed186eb1fecde7dd418d2b4ceac9543d69eac995f9d1c5a25d13827f5a",
        ),
        (
            "e_t41dudSc6dXymoEa8iNw",
            "a5b456119f2ae91f7e08a194d7c8a3d0df2c30212d0cb17fb4d64346673e860b",
        ),
    ];

    /// A digest of everything about a badge that both sites must say
    /// identically.
    ///
    /// **The format is a cross-repository contract**: orrery computes the
    /// same digest over its own registry in `src/badgedata.rs`, and the two
    /// are compared by eye through the pinned constants above, so any change
    /// here has to be mirrored there or the digests stop meaning anything.
    ///
    /// Each field is written as its label, `0x1f`, its value, `0x1e`. A
    /// missing optional value is the single byte `0x00`, which cannot occur
    /// in the text itself, so "absent" and "empty" stay distinguishable.
    /// Lists are preceded by their length, so reordering or truncating them
    /// changes the digest. Deliberately absent: the image URL and path, the
    /// issuer, and the badge page's own address, which are per-site and
    /// *should* differ.
    fn badge_text_digest(badge: Badge) -> String {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();

        let mut field = |label: &str, value: Option<&str>| {
            hasher.update(label.as_bytes());
            hasher.update([0x1f]);

            match value {
                Some(value) => hasher.update(value.as_bytes()),
                None => hasher.update([0x00]),
            }

            hasher.update([0x1e]);
        };

        field("badgeclass", Some(badge.badgeclass()));
        field("name", Some(badge.name()));
        field("description", Some(badge.description()));
        field("criteria_url", Some(badge.criteria_url()));
        field("criteria_text", Some(badge.criteria_text()));
        field("earn_url", badge.earn_url());

        field("tags", Some(&badge.tags().len().to_string()));
        for tag in badge.tags() {
            field("tag", Some(tag));
        }

        field("alignments", Some(&badge.alignments().len().to_string()));
        for alignment in badge.alignments() {
            field("alignment_name", Some(alignment.name));
            field("alignment_url", Some(alignment.url));
            field("alignment_description", alignment.description);
            field("alignment_framework", alignment.framework);
            field("alignment_code", alignment.code);
        }

        hex::encode(hasher.finalize())
    }

    /// Badge text is duplicated in orrery, which issues the same badges to
    /// the same people, and the two copies have drifted before: placeholder
    /// descriptions, invented tags, a skill missing from one side.
    ///
    /// Nothing can compare the repositories at test time, so each pins what
    /// its own text hashes to. Changing badge text here fails this test with
    /// the value to paste in -- and that is the moment to make the matching
    /// change in orrery, whose test will then fail with the same value.
    #[test]
    fn badge_text_matches_the_digests_orrery_is_pinned_to() {
        let mut current: Vec<(String, String)> = Badge::ALL
            .into_iter()
            .map(|badge| (badge.badgeclass().to_string(), badge_text_digest(badge)))
            .collect();

        current.sort();

        let pinned: Vec<(String, String)> = BADGE_TEXT_DIGESTS
            .iter()
            .map(|(badgeclass, digest)| (badgeclass.to_string(), digest.to_string()))
            .collect();

        if current != pinned {
            let listing: String = current
                .iter()
                .map(|(badgeclass, digest)| format!("        (\"{badgeclass}\", \"{digest}\"),\n"))
                .collect();

            panic!(
                "badge text has changed. Update BADGE_TEXT_DIGESTS here and the \
                 matching constant in orrery's src/badgedata.rs, and make the \
                 same text change there:\n{listing}"
            );
        }
    }
}
