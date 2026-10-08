//! An exchange's engagement figures, counted the way orrery and hydra agree
//! to count them, over events of both origins: hydra's own, which carry
//! analytics ids, and orrery's oldest, which do not.

use anyhow::Result;
use common::message::log::ExchangeEngagementRequest;
use log_service::database::storage::Database;
use log_service::database::tables::{ExchangeEventRow, ParticipationRow};
use log_service::exchange_engagement::engagement_stats;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{Duration, sleep};

const EXCHANGE: &str = "01M2MR8QB0GR6KAPXE7ZNSP63Y";
const OTHER: &str = "01JS1MRVX838H7WZBCCABDKW90";
const LISTED: &str = "01K000000000000000000PRJ01";
const ALSO_LISTED: &str = "01K000000000000000000PRJ02";
const UNLISTED: &str = "01K000000000000000000PRJ03";

fn event(
    exchange: &str,
    when: &str,
    device: &str,
    interaction: &str,
    page: &str,
    page_id: Option<&str>,
    action: &str,
) -> ExchangeEventRow {
    ExchangeEventRow {
        when: when.into(),
        event_id: ulid::Ulid::new().to_string(),
        exchange_id: exchange.into(),
        device_id: device.into(),
        interaction_id: interaction.into(),
        user_id: None,
        page: page.into(),
        page_id: page_id.map(str::to_string),
        action: action.into(),
        extra: None,
    }
}

fn participation(project: &str, when: &str) -> ParticipationRow {
    ParticipationRow {
        origin: 0,
        when: when.into(),
        duration: 1.0,
        where_point: None,
        magnitude: 1,
        authenticated: false,
        extra: String::new(),
        fidelity: 0.5,
        entity_id: project.into(),
        user_id: None,
        task_name: None,
    }
}

async fn database() -> Result<(TempDir, Arc<Database>)> {
    let temp_dir = TempDir::new()?;
    let database = Arc::new(Database::new(temp_dir.path().to_string_lossy().to_string()).await?);

    const DAY: &str = "2026-10-09T10:00:00Z";
    const LATER: &str = "2026-10-09T11:00:00Z";

    for row in [
        // One device, one sitting: the home page and a project.
        event(EXCHANGE, DAY, "dev-a", "int-1", "home", None, "view"),
        event(
            EXCHANGE,
            DAY,
            "dev-a",
            "int-1",
            "project",
            Some(LISTED),
            "view",
        ),
        // The same device back later, a second visit.
        event(
            EXCHANGE,
            LATER,
            "dev-a",
            "int-2",
            "project",
            Some(LISTED),
            "view",
        ),
        // A second device saves a project and follows it out.
        event(
            EXCHANGE,
            DAY,
            "dev-b",
            "int-3",
            "project",
            Some(ALSO_LISTED),
            "view",
        ),
        event(
            EXCHANGE,
            DAY,
            "dev-b",
            "int-3",
            "project",
            Some(LISTED),
            "save",
        ),
        event(
            EXCHANGE,
            DAY,
            "dev-b",
            "int-3",
            "project",
            Some(LISTED),
            "visit",
        ),
        // The org page is not one of the exchange's views.
        event(EXCHANGE, DAY, "dev-a", "int-4", "org", None, "view"),
        // An orrery event from before the analytics ids were kept.
        event(
            EXCHANGE,
            "2025-05-01T00:00:00Z",
            "",
            "",
            "project",
            Some(ALSO_LISTED),
            "view",
        ),
        // Another exchange's traffic.
        event(OTHER, DAY, "dev-c", "int-5", "home", None, "view"),
    ] {
        database.exchange_events.insert(row).await?;
    }

    for project in [LISTED, LISTED, LISTED, UNLISTED, UNLISTED] {
        database
            .participation
            .insert(participation(project, DAY))
            .await?;
    }

    // Writes land in the WAL; give them a moment to be readable.
    sleep(Duration::from_millis(100)).await;

    Ok((temp_dir, database))
}

fn request(year: Option<i32>) -> ExchangeEngagementRequest {
    ExchangeEngagementRequest {
        exchange_id: EXCHANGE.into(),
        year,
        project_ids: vec![LISTED.into(), ALSO_LISTED.into()],
    }
}

#[tokio::test]
async fn figures_count_what_both_codebases_agreed_on() -> Result<()> {
    let (_dir, database) = database().await?;

    let stats = engagement_stats(&database.context(), &request(None)).await?;

    assert_eq!(
        stats.views, 5,
        "home and project views, the untracked one included"
    );
    assert_eq!(
        stats.exchange_visits, 3,
        "distinct tracked interactions among them"
    );
    assert_eq!(stats.visitors, 2, "distinct tracked devices");
    assert_eq!(stats.saves, 1);
    assert_eq!(stats.referrals, 1);
    assert_eq!(
        stats.contributions, 3,
        "repetitions on listed projects only"
    );
    assert_eq!(stats.tracked_since.as_deref(), Some("2026-10-09T10:00:00Z"));

    assert_eq!(stats.top_referrals.len(), 1);
    assert_eq!(stats.top_referrals[0].project_id, LISTED);

    let views: Vec<_> = stats
        .top_views
        .iter()
        .map(|top| (top.project_id.as_str(), top.count))
        .collect();
    assert!(views.contains(&(LISTED, 2)));
    assert!(views.contains(&(ALSO_LISTED, 2)));

    Ok(())
}

#[tokio::test]
async fn a_year_before_tracking_has_views_but_no_visits() -> Result<()> {
    let (_dir, database) = database().await?;

    let stats = engagement_stats(&database.context(), &request(Some(2025))).await?;

    assert_eq!(stats.views, 1);
    assert_eq!(stats.exchange_visits, 0);
    assert_eq!(stats.visitors, 0);

    Ok(())
}

#[tokio::test]
async fn ids_that_are_not_ulids_are_refused() -> Result<()> {
    let (_dir, database) = database().await?;

    let mut bad = request(None);
    bad.project_ids.push("x' OR '1'='1".into());

    assert!(engagement_stats(&database.context(), &bad).await.is_err());

    Ok(())
}
