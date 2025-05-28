use anyhow::Error;
use common::shared::nats;
use futures::StreamExt;

pub mod all;
pub mod field;

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

/// Launch as a separate task to receive and process project updates over NATS
pub async fn receive_project_updates() -> Result<(), Error> {
    let client = nats();

    let mut updates = client.subscribe("frontend.project.>").await?;

    while let Some(msg) = updates.next().await {
        match msg.subject.as_str() {
            "frontend.project.save.all" => all::project_all_save(msg).await?,
            "frontend.project.save.field" => field::project_field_save(msg).await?,
            subj => eprintln!("Unrecognized frontend.project.> subject {}", subj),
        }
    }

    Ok(())
}
