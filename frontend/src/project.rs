pub mod all;
pub mod field;
pub mod field_source;

#[derive(Clone, Copy, Default, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum ProjectStatus {
    Pending,
    #[default]
    Active,
    #[sqlx(rename = "Periodically Active")]
    PeriodicallyActive,
    Hiatus,
    Complete,
    Hidden,
    Expired,
}

#[derive(Clone, Copy, Default, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum ProjectPromotionTime {
    #[default]
    #[sqlx(rename = "Promote at any time")]
    PromoteAtAnyTime,
    #[sqlx(rename = "Do not promote")]
    DoNotPromote,
    #[sqlx(rename = "Prefer promotion on the target date")]
    PreferOnTargetDate,
}

#[derive(Clone, Copy, Default, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum ProjectParticipationDegree {
    #[default]
    Collegial,
    #[sqlx(rename = "Co-Created")]
    CoCreated,
    Collaborative,
    Contributory,
    Contractual,
}
