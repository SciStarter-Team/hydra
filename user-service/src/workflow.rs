// Workflow progress tracking for Articulate training programs

use async_nats::service::Request;
use common::badge::Badge;
use common::message::user::{
    FoundationsStateEntry, FunnelStage, TrainingStatesResponse, UserAndBadge, WorkflowFunnel,
    WorkflowFunnelResponse, WorkflowProgressRequest, WorkflowProgressResponse,
    WorkflowStatusRequest, WorkflowStatusResponse, WorkflowTutorialCountRequest,
    WorkflowTutorialCountResponse, WorkflowType,
};
use common::{
    service::{TriggerEvent, decode_structured_message, send, trigger_email},
    shared::db,
};
use ulid::Ulid;

/// Record workflow progress step completion
pub async fn record_progress(req: &Request) -> Result<WorkflowProgressResponse, anyhow::Error> {
    let request: WorkflowProgressRequest = decode_structured_message(req.message.clone())?;

    let now = jiff::Zoned::now().to_string();

    let badge_granted = match request.workflow {
        WorkflowType::Foundations => {
            update_foundations_progress(&request.user_id, &request.step, &now).await?
        }
        WorkflowType::LibrariesHubs => {
            update_libraries_hubs_progress(&request.user_id, &request.step, &now).await?
        }
        WorkflowType::DataLiteracy => {
            update_data_literacy_progress(&request.user_id, &request.step, &now).await?
        }
        WorkflowType::HigherEd => {
            update_higher_ed_progress(&request.user_id, &request.step, &now).await?
        }
        WorkflowType::DataEthics => {
            update_data_ethics_progress(&request.user_id, &request.step, &now).await?
        }
        WorkflowType::Ideal => update_ideal_progress(&request.user_id, &request.step, &now).await?,
        WorkflowType::Ambassador => {
            update_ambassador_progress(&request.user_id, &request.step, &now).await?
        }
    };

    // Notify any open training page for this user (over the per-user SSE
    // channel) that their progress changed, so it re-renders live. The event
    // name is the workflow slug; the payload is just a trigger. Fire-and-forget
    // — a failed notification must not fail progress recording.
    if let Ok(user_id) = Ulid::from_string(&request.user_id) {
        let _ = common::channel::send_to_user(
            user_id,
            "training",
            common::channel::Event::new(request.workflow.slug(), ""),
        )
        .await;
    }

    Ok(WorkflowProgressResponse {
        success: true,
        badge_granted,
    })
}

/// Update Foundations of Citizen Science progress
async fn update_foundations_progress(
    user_id: &str,
    step: &str,
    now: &str,
) -> Result<bool, anyhow::Error> {
    // Ensure row exists
    sqlx::query!(
        "INSERT OR IGNORE INTO foundations_progress (user_id) VALUES (?)",
        user_id
    )
    .execute(db())
    .await?;

    let mut badge_granted = false;
    let user_ulid = Ulid::from_string(user_id)?;

    match step {
        "tutorial" => {
            let result = sqlx::query!(
                "UPDATE foundations_progress SET hour1_completed_at = ? WHERE user_id = ? AND hour1_completed_at IS NULL",
                now, user_id
            )
            .execute(db())
            .await?;

            // Trigger email if this is a new completion
            if result.rows_affected() > 0 {
                trigger_email(TriggerEvent::FoundationsTutorial, user_ulid, None).await?;
            }
        }
        "squirrel" => {
            let result = sqlx::query!(
                "UPDATE foundations_progress SET squirrel_completed_at = ? WHERE user_id = ? AND squirrel_completed_at IS NULL",
                now, user_id
            )
            .execute(db())
            .await?;

            // Trigger email if this is a new completion
            if result.rows_affected() > 0 {
                trigger_email(TriggerEvent::FoundationsSquirrel, user_ulid, None).await?;
            }

            // Check if badge should be granted (need tutorial + 2 projects)
            badge_granted = check_foundations_badge(user_id).await?;
        }
        "stall" => {
            let result = sqlx::query!(
                "UPDATE foundations_progress SET stall_completed_at = ? WHERE user_id = ? AND stall_completed_at IS NULL",
                now, user_id
            )
            .execute(db())
            .await?;

            // Trigger email if this is a new completion
            if result.rows_affected() > 0 {
                trigger_email(TriggerEvent::FoundationsStall, user_ulid, None).await?;
            }

            badge_granted = check_foundations_badge(user_id).await?;
        }
        "debris" => {
            let result = sqlx::query!(
                "UPDATE foundations_progress SET debris_completed_at = ? WHERE user_id = ? AND debris_completed_at IS NULL",
                now, user_id
            )
            .execute(db())
            .await?;

            // Trigger email if this is a new completion
            if result.rows_affected() > 0 {
                trigger_email(TriggerEvent::FoundationsDebris, user_ulid, None).await?;
            }

            badge_granted = check_foundations_badge(user_id).await?;
        }
        _ => {}
    }

    Ok(badge_granted)
}

/// Check if Foundations badge should be granted (hour1 + 2 of 3 projects)
async fn check_foundations_badge(user_id: &str) -> Result<bool, anyhow::Error> {
    let row = sqlx::query!(
        r#"SELECT
            hour1_completed_at IS NOT NULL as "has_hour1: bool",
            (squirrel_completed_at IS NOT NULL) as "has_squirrel: bool",
            (stall_completed_at IS NOT NULL) as "has_stall: bool",
            (debris_completed_at IS NOT NULL) as "has_debris: bool",
            badge_granted_at
        FROM foundations_progress
        WHERE user_id = ?"#,
        user_id
    )
    .fetch_one(db())
    .await?;

    // Badge already granted
    if row.badge_granted_at.is_some() {
        return Ok(false);
    }

    // Check: hour1 + at least 2 of the 3 projects
    let project_count = [row.has_squirrel, row.has_stall, row.has_debris]
        .iter()
        .filter(|&&x| x)
        .count();

    if row.has_hour1 && project_count >= 2 {
        let now = jiff::Zoned::now().to_string();
        sqlx::query!(
            "UPDATE foundations_progress SET badge_granted_at = ? WHERE user_id = ?",
            now,
            user_id
        )
        .execute(db())
        .await?;

        // Add user tag
        send(
            "user.tags.add",
            (user_id.to_string(), "foundations-badge".to_string()),
        )
        .await?;

        // Grant badge
        let user_ulid = Ulid::from_string(user_id)?;
        send(
            "user.badges.grant",
            UserAndBadge {
                user_id: user_ulid,
                badge: Badge::FoundationsOfCitizenScience,
            },
        )
        .await?;

        // Trigger email notification
        trigger_email(TriggerEvent::BadgeFoundations, user_ulid, None).await?;

        return Ok(true);
    }

    Ok(false)
}

/// Update Libraries as Community Hubs progress
async fn update_libraries_hubs_progress(
    user_id: &str,
    step: &str,
    now: &str,
) -> Result<bool, anyhow::Error> {
    sqlx::query!(
        "INSERT OR IGNORE INTO libraries_hubs_progress (user_id) VALUES (?)",
        user_id
    )
    .execute(db())
    .await?;

    let user_ulid = Ulid::from_string(user_id)?;

    match step {
        "tutorial" => {
            let result = sqlx::query!(
                "UPDATE libraries_hubs_progress SET hour2_completed_at = ? WHERE user_id = ? AND hour2_completed_at IS NULL",
                now, user_id
            )
            .execute(db())
            .await?;

            // Trigger email if this is a new completion
            if result.rows_affected() > 0 {
                trigger_email(TriggerEvent::LibrariesHubsTutorial, user_ulid, None).await?;
            }
        }
        "supplemental" => {
            let result = sqlx::query!(
                "UPDATE libraries_hubs_progress SET supplemental_signup_at = ? WHERE user_id = ? AND supplemental_signup_at IS NULL",
                now, user_id
            )
            .execute(db())
            .await?;

            // Trigger email if this is a new completion
            if result.rows_affected() > 0 {
                trigger_email(TriggerEvent::LibrariesHubsSupplemental, user_ulid, None).await?;
            }

            // Grant badge if both steps complete
            return check_libraries_hubs_badge(user_id).await;
        }
        _ => {}
    }

    Ok(false)
}

/// Check if Libraries Hubs badge should be granted
async fn check_libraries_hubs_badge(user_id: &str) -> Result<bool, anyhow::Error> {
    let row = sqlx::query!(
        r#"SELECT
            hour2_completed_at IS NOT NULL as "has_hour2: bool",
            supplemental_signup_at IS NOT NULL as "has_supplemental: bool",
            badge_granted_at
        FROM libraries_hubs_progress
        WHERE user_id = ?"#,
        user_id
    )
    .fetch_one(db())
    .await?;

    if row.badge_granted_at.is_some() {
        return Ok(false);
    }

    if row.has_hour2 && row.has_supplemental {
        let now = jiff::Zoned::now().to_string();
        sqlx::query!(
            "UPDATE libraries_hubs_progress SET badge_granted_at = ? WHERE user_id = ?",
            now,
            user_id
        )
        .execute(db())
        .await?;

        // Add user tag
        send(
            "user.tags.add",
            (user_id.to_string(), "libraries-hubs-badge".to_string()),
        )
        .await?;

        // Grant badge
        let user_ulid = Ulid::from_string(user_id)?;
        send(
            "user.badges.grant",
            UserAndBadge {
                user_id: user_ulid,
                badge: Badge::LibrariesAsHubsForCitizenScience,
            },
        )
        .await?;

        // Trigger email notification
        trigger_email(TriggerEvent::BadgeLibrariesHubs, user_ulid, None).await?;

        return Ok(true);
    }

    Ok(false)
}

/// Update Data Literacy progress (single step - grants badge immediately)
async fn update_data_literacy_progress(
    user_id: &str,
    step: &str,
    now: &str,
) -> Result<bool, anyhow::Error> {
    if step != "tutorial" {
        return Ok(false);
    }

    let result = sqlx::query!(
        "INSERT OR IGNORE INTO data_literacy_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
        user_id, now, now
    )
    .execute(db())
    .await?;

    // Only grant badge if this is a new completion (rows_affected > 0)
    if result.rows_affected() > 0 {
        // Add user tag
        send(
            "user.tags.add",
            (user_id.to_string(), "data-literacy-badge".to_string()),
        )
        .await?;

        // Grant badge
        let user_ulid = Ulid::from_string(user_id)?;
        send(
            "user.badges.grant",
            UserAndBadge {
                user_id: user_ulid,
                badge: Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience,
            },
        )
        .await?;

        // Trigger email notification
        trigger_email(TriggerEvent::BadgeDataLiteracy, user_ulid, None).await?;

        Ok(true)
    } else {
        Ok(false)
    }
}

/// Update Higher Ed progress
/// Higher Education is a sequential three-step workflow (tutorial → list →
/// explore), matching orrery's HigherEdState. Each step is only recorded once
/// its predecessor is complete; completing `explore` grants the badge. `list`
/// and `explore` never create a row — only a participant who has completed the
/// tutorial can advance — so list/add-to-list actions by non-participants are
/// harmless no-ops.
async fn update_higher_ed_progress(
    user_id: &str,
    step: &str,
    now: &str,
) -> Result<bool, anyhow::Error> {
    match step {
        "tutorial" => {
            sqlx::query!(
                "INSERT OR IGNORE INTO higher_ed_progress (user_id) VALUES (?)",
                user_id
            )
            .execute(db())
            .await?;
            sqlx::query!(
                "UPDATE higher_ed_progress SET tutorial_completed_at = COALESCE(tutorial_completed_at, ?) WHERE user_id = ?",
                now,
                user_id
            )
            .execute(db())
            .await?;
            Ok(false)
        }
        "list" => {
            sqlx::query!(
                "UPDATE higher_ed_progress SET list_completed_at = ?
                 WHERE user_id = ? AND tutorial_completed_at IS NOT NULL AND list_completed_at IS NULL",
                now,
                user_id
            )
            .execute(db())
            .await?;
            Ok(false)
        }
        "explore" => {
            let result = sqlx::query!(
                "UPDATE higher_ed_progress
                 SET explore_completed_at = ?, badge_granted_at = COALESCE(badge_granted_at, ?)
                 WHERE user_id = ? AND list_completed_at IS NOT NULL AND explore_completed_at IS NULL",
                now,
                now,
                user_id
            )
            .execute(db())
            .await?;

            // Reaching explore (with tutorial and list already done) grants the badge.
            if result.rows_affected() > 0 {
                send(
                    "user.tags.add",
                    (user_id.to_string(), "higher-ed-badge".to_string()),
                )
                .await?;

                let user_ulid = Ulid::from_string(user_id)?;
                send(
                    "user.badges.grant",
                    UserAndBadge {
                        user_id: user_ulid,
                        badge: Badge::TeachingInHigherEducationWithCitizenScience,
                    },
                )
                .await?;

                trigger_email(TriggerEvent::BadgeHigherEd, user_ulid, None).await?;

                Ok(true)
            } else {
                Ok(false)
            }
        }
        _ => Ok(false),
    }
}

/// Update Data Ethics progress (single step - grants badge immediately)
async fn update_data_ethics_progress(
    user_id: &str,
    step: &str,
    now: &str,
) -> Result<bool, anyhow::Error> {
    if step != "tutorial" {
        return Ok(false);
    }

    let result = sqlx::query!(
        "INSERT OR IGNORE INTO data_ethics_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
        user_id, now, now
    )
    .execute(db())
    .await?;

    // Only grant badge if this is a new completion
    if result.rows_affected() > 0 {
        // Add user tag
        send(
            "user.tags.add",
            (user_id.to_string(), "data-ethics-badge".to_string()),
        )
        .await?;

        // Grant badge
        let user_ulid = Ulid::from_string(user_id)?;
        send(
            "user.badges.grant",
            UserAndBadge {
                user_id: user_ulid,
                badge: Badge::DataEthicsForPractitioners,
            },
        )
        .await?;

        // Trigger email notification
        trigger_email(TriggerEvent::BadgeDataEthics, user_ulid, None).await?;

        Ok(true)
    } else {
        Ok(false)
    }
}

/// Update IDEAL progress (single step - grants badge immediately)
async fn update_ideal_progress(
    user_id: &str,
    step: &str,
    now: &str,
) -> Result<bool, anyhow::Error> {
    if step != "tutorial" {
        return Ok(false);
    }

    let result = sqlx::query!(
        "INSERT OR IGNORE INTO ideal_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
        user_id, now, now
    )
    .execute(db())
    .await?;

    // Only grant badge if this is a new completion
    if result.rows_affected() > 0 {
        // Add user tag
        send(
            "user.tags.add",
            (user_id.to_string(), "ideal-badge".to_string()),
        )
        .await?;

        // Grant badge
        let user_ulid = Ulid::from_string(user_id)?;
        send(
            "user.badges.grant",
            UserAndBadge {
                user_id: user_ulid,
                badge: Badge::IdealParticipatoryScience,
            },
        )
        .await?;

        // Trigger email notification
        trigger_email(TriggerEvent::BadgeIdeal, user_ulid, None).await?;

        Ok(true)
    } else {
        Ok(false)
    }
}

/// Update Ambassador progress
async fn update_ambassador_progress(
    user_id: &str,
    step: &str,
    now: &str,
) -> Result<bool, anyhow::Error> {
    if step != "tutorial" {
        return Ok(false);
    }

    // Completing the Champion tutorial earns the Champion *certificate* (orrery's
    // AmbassadorState.tutorial_enter), NOT the Certified Ambassador badge — that
    // is earned later via the application + verification (granted through the
    // orrery state sync's "verified" state). So we record the tutorial timestamp,
    // tag the champion, and issue the certificate; no badge is granted here.
    sqlx::query!(
        "INSERT OR IGNORE INTO ambassador_progress (user_id) VALUES (?)",
        user_id
    )
    .execute(db())
    .await?;
    let result = sqlx::query!(
        "UPDATE ambassador_progress SET tutorial_completed_at = ? WHERE user_id = ? AND tutorial_completed_at IS NULL",
        now,
        user_id
    )
    .execute(db())
    .await?;

    if result.rows_affected() > 0 {
        send(
            "user.tags.add",
            (
                user_id.to_string(),
                "scistarter-ambassador-training-complete".to_string(),
            ),
        )
        .await?;

        let user_ulid = Ulid::from_string(user_id)?;
        crate::certificates::create_certificate(user_ulid, String::new(), "champion".to_string())
            .await?;
    }

    // The tutorial grants a certificate, not a badge.
    Ok(false)
}

/// Get workflow status for a user
pub async fn get_status(req: &Request) -> Result<WorkflowStatusResponse, anyhow::Error> {
    let request: WorkflowStatusRequest = decode_structured_message(req.message.clone())?;

    match request.workflow {
        WorkflowType::Foundations => get_foundations_status(&request.user_id).await,
        WorkflowType::LibrariesHubs => get_libraries_hubs_status(&request.user_id).await,
        WorkflowType::DataLiteracy => get_data_literacy_status(&request.user_id).await,
        WorkflowType::HigherEd => get_higher_ed_status(&request.user_id).await,
        WorkflowType::DataEthics => get_data_ethics_status(&request.user_id).await,
        WorkflowType::Ideal => get_ideal_status(&request.user_id).await,
        WorkflowType::Ambassador => get_ambassador_status(&request.user_id).await,
    }
}

/// Platform-wide count of users who have completed a workflow's tutorial
/// step. Drives the motivational "Tutorials Completed to Date" figure on
/// each training page. Each workflow's tutorial lives in a differently named
/// column, so the queries are spelled out per workflow rather than generic.
pub async fn tutorial_count(req: &Request) -> Result<WorkflowTutorialCountResponse, anyhow::Error> {
    let request: WorkflowTutorialCountRequest = decode_structured_message(req.message.clone())?;

    let count = match request.workflow {
        WorkflowType::Foundations => sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM foundations_progress WHERE hour1_completed_at IS NOT NULL"#
        )
        .fetch_one(db())
        .await?,
        WorkflowType::LibrariesHubs => sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM libraries_hubs_progress WHERE hour2_completed_at IS NOT NULL"#
        )
        .fetch_one(db())
        .await?,
        WorkflowType::DataLiteracy => sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM data_literacy_progress WHERE tutorial_completed_at IS NOT NULL"#
        )
        .fetch_one(db())
        .await?,
        WorkflowType::HigherEd => sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM higher_ed_progress WHERE tutorial_completed_at IS NOT NULL"#
        )
        .fetch_one(db())
        .await?,
        WorkflowType::DataEthics => sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM data_ethics_progress WHERE tutorial_completed_at IS NOT NULL"#
        )
        .fetch_one(db())
        .await?,
        WorkflowType::Ideal => sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM ideal_progress WHERE tutorial_completed_at IS NOT NULL"#
        )
        .fetch_one(db())
        .await?,
        WorkflowType::Ambassador => sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM ambassador_progress WHERE tutorial_completed_at IS NOT NULL"#
        )
        .fetch_one(db())
        .await?,
    };

    Ok(WorkflowTutorialCountResponse { count })
}

async fn get_foundations_status(user_id: &str) -> Result<WorkflowStatusResponse, anyhow::Error> {
    let row = sqlx::query!(
        "SELECT hour1_completed_at, squirrel_completed_at, stall_completed_at, debris_completed_at, badge_granted_at
         FROM foundations_progress WHERE user_id = ?",
        user_id
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        return Ok(WorkflowStatusResponse {
            current_step: None,
            completed_steps: vec![],
            badge_granted: false,
        });
    };

    let mut completed_steps = Vec::new();
    if row.hour1_completed_at.is_some() {
        completed_steps.push("tutorial".to_string());
    }
    if row.squirrel_completed_at.is_some() {
        completed_steps.push("squirrel".to_string());
    }
    if row.stall_completed_at.is_some() {
        completed_steps.push("stall".to_string());
    }
    if row.debris_completed_at.is_some() {
        completed_steps.push("debris".to_string());
    }

    let current_step = completed_steps.last().cloned();

    Ok(WorkflowStatusResponse {
        current_step,
        completed_steps,
        badge_granted: row.badge_granted_at.is_some(),
    })
}

async fn get_libraries_hubs_status(user_id: &str) -> Result<WorkflowStatusResponse, anyhow::Error> {
    let row = sqlx::query!(
        "SELECT hour2_completed_at, supplemental_signup_at, badge_granted_at
         FROM libraries_hubs_progress WHERE user_id = ?",
        user_id
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        return Ok(WorkflowStatusResponse {
            current_step: None,
            completed_steps: vec![],
            badge_granted: false,
        });
    };

    let mut completed_steps = Vec::new();
    if row.hour2_completed_at.is_some() {
        completed_steps.push("tutorial".to_string());
    }
    if row.supplemental_signup_at.is_some() {
        completed_steps.push("supplemental".to_string());
    }

    let current_step = completed_steps.last().cloned();

    Ok(WorkflowStatusResponse {
        current_step,
        completed_steps,
        badge_granted: row.badge_granted_at.is_some(),
    })
}

async fn get_data_literacy_status(user_id: &str) -> Result<WorkflowStatusResponse, anyhow::Error> {
    let row = sqlx::query!(
        "SELECT tutorial_completed_at, badge_granted_at FROM data_literacy_progress WHERE user_id = ?",
        user_id
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        return Ok(WorkflowStatusResponse {
            current_step: None,
            completed_steps: vec![],
            badge_granted: false,
        });
    };

    let completed_steps = if row.tutorial_completed_at.is_some() {
        vec!["tutorial".to_string()]
    } else {
        vec![]
    };

    Ok(WorkflowStatusResponse {
        current_step: completed_steps.last().cloned(),
        completed_steps,
        badge_granted: row.badge_granted_at.is_some(),
    })
}

async fn get_higher_ed_status(user_id: &str) -> Result<WorkflowStatusResponse, anyhow::Error> {
    let row = sqlx::query!(
        "SELECT tutorial_completed_at, list_completed_at, explore_completed_at, badge_granted_at
         FROM higher_ed_progress WHERE user_id = ?",
        user_id
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        return Ok(WorkflowStatusResponse {
            current_step: None,
            completed_steps: vec![],
            badge_granted: false,
        });
    };

    let mut completed_steps = Vec::new();
    if row.tutorial_completed_at.is_some() {
        completed_steps.push("tutorial".to_string());
    }
    if row.list_completed_at.is_some() {
        completed_steps.push("list".to_string());
    }
    if row.explore_completed_at.is_some() {
        completed_steps.push("explore".to_string());
    }

    Ok(WorkflowStatusResponse {
        current_step: completed_steps.last().cloned(),
        completed_steps,
        badge_granted: row.badge_granted_at.is_some(),
    })
}

async fn get_data_ethics_status(user_id: &str) -> Result<WorkflowStatusResponse, anyhow::Error> {
    let row = sqlx::query!(
        "SELECT tutorial_completed_at, badge_granted_at FROM data_ethics_progress WHERE user_id = ?",
        user_id
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        return Ok(WorkflowStatusResponse {
            current_step: None,
            completed_steps: vec![],
            badge_granted: false,
        });
    };

    let completed_steps = if row.tutorial_completed_at.is_some() {
        vec!["tutorial".to_string()]
    } else {
        vec![]
    };

    Ok(WorkflowStatusResponse {
        current_step: completed_steps.last().cloned(),
        completed_steps,
        badge_granted: row.badge_granted_at.is_some(),
    })
}

async fn get_ideal_status(user_id: &str) -> Result<WorkflowStatusResponse, anyhow::Error> {
    let row = sqlx::query!(
        "SELECT tutorial_completed_at, badge_granted_at FROM ideal_progress WHERE user_id = ?",
        user_id
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        return Ok(WorkflowStatusResponse {
            current_step: None,
            completed_steps: vec![],
            badge_granted: false,
        });
    };

    let completed_steps = if row.tutorial_completed_at.is_some() {
        vec!["tutorial".to_string()]
    } else {
        vec![]
    };

    Ok(WorkflowStatusResponse {
        current_step: completed_steps.last().cloned(),
        completed_steps,
        badge_granted: row.badge_granted_at.is_some(),
    })
}

async fn get_ambassador_status(user_id: &str) -> Result<WorkflowStatusResponse, anyhow::Error> {
    let row = sqlx::query!(
        "SELECT tutorial_completed_at, badge_granted_at FROM ambassador_progress WHERE user_id = ?",
        user_id
    )
    .fetch_optional(db())
    .await?;

    let Some(row) = row else {
        return Ok(WorkflowStatusResponse {
            current_step: None,
            completed_steps: vec![],
            badge_granted: false,
        });
    };

    let completed_steps = if row.tutorial_completed_at.is_some() {
        vec!["tutorial".to_string()]
    } else {
        vec![]
    };

    Ok(WorkflowStatusResponse {
        current_step: completed_steps.last().cloned(),
        completed_steps,
        badge_granted: row.badge_granted_at.is_some(),
    })
}

fn stage(label: &str, count: i64) -> FunnelStage {
    FunnelStage {
        label: label.to_string(),
        count,
    }
}

/// Derive an orrery-style Foundations state name from the completion flags.
/// Returns None for the "initial" state (no first hour yet), which the
/// training-state export excludes. hydra tracks independent step timestamps
/// rather than a single state-machine node, so the deeper orrery nodes between
/// "bookmark2" and "badge" are collapsed to those representable milestones.
fn derive_foundations_state(
    hour1: bool,
    squirrel: bool,
    stall: bool,
    debris: bool,
    badge: bool,
) -> Option<&'static str> {
    if !hour1 {
        return None;
    }
    if badge {
        return Some("badge");
    }
    let projects = [squirrel, stall, debris].iter().filter(|x| **x).count();
    if projects >= 2 {
        return Some("bookmark2");
    }
    if squirrel {
        return Some("squirrel");
    }
    if stall {
        return Some("stall");
    }
    if debris {
        return Some("debris");
    }
    Some("hour1")
}

/// Export every user's current Foundations state (opaque id + state), excluding
/// users still in the initial state. Backs /api/training-state.json.
pub async fn training_states(_req: &Request) -> Result<TrainingStatesResponse, anyhow::Error> {
    let rows = sqlx::query!(
        r#"SELECT u.opaque_id AS "opaque_id?",
            fp.hour1_completed_at, fp.squirrel_completed_at, fp.stall_completed_at,
            fp.debris_completed_at, fp.badge_granted_at
           FROM foundations_progress fp
           JOIN users u ON u.user_id = fp.user_id
           WHERE fp.hour1_completed_at IS NOT NULL"#
    )
    .fetch_all(db())
    .await?;

    let mut states = Vec::new();
    for row in rows {
        let Some(opaque_id) = row.opaque_id else {
            continue;
        };
        if let Some(state) = derive_foundations_state(
            row.hour1_completed_at.is_some(),
            row.squirrel_completed_at.is_some(),
            row.stall_completed_at.is_some(),
            row.debris_completed_at.is_some(),
            row.badge_granted_at.is_some(),
        ) {
            states.push(FoundationsStateEntry {
                person: opaque_id,
                state: state.to_string(),
            });
        }
    }

    Ok(TrainingStatesResponse { states })
}

/// Platform-wide engagement funnel for every training workflow. Each
/// `*_progress` row is one user who started; each `*_completed_at` column is
/// non-null once that step is reached, so `COUNT(col)` is the number of users
/// who got that far (the funnel is naturally cumulative since steps complete in
/// order). Powers the training-analytics staff report.
pub async fn funnel(_req: &Request) -> Result<WorkflowFunnelResponse, anyhow::Error> {
    let mut funnels = Vec::new();

    let f = sqlx::query!(
        r#"SELECT
            COUNT(*) AS "started!: i64",
            COUNT(hour1_completed_at) AS "hour1!: i64",
            COUNT(squirrel_completed_at) AS "squirrel!: i64",
            COUNT(stall_completed_at) AS "stall!: i64",
            COUNT(debris_completed_at) AS "debris!: i64",
            COUNT(badge_granted_at) AS "badge!: i64"
           FROM foundations_progress"#
    )
    .fetch_one(db())
    .await?;
    funnels.push(WorkflowFunnel {
        name: "Foundations".to_string(),
        stages: vec![
            stage("Started", f.started),
            stage("First hour", f.hour1),
            stage("Project Squirrel", f.squirrel),
            stage("Stall Catchers", f.stall),
            stage("Debris Tracker", f.debris),
            stage("Badge earned", f.badge),
        ],
        note: Some(
            "The badge requires the first hour plus any two of the three projects, \
             so the three project rows are alternatives rather than sequential steps."
                .to_string(),
        ),
    });

    let l = sqlx::query!(
        r#"SELECT
            COUNT(*) AS "started!: i64",
            COUNT(hour2_completed_at) AS "hour2!: i64",
            COUNT(supplemental_signup_at) AS "supplemental!: i64",
            COUNT(badge_granted_at) AS "badge!: i64"
           FROM libraries_hubs_progress"#
    )
    .fetch_one(db())
    .await?;
    funnels.push(WorkflowFunnel {
        name: "Libraries as Hubs".to_string(),
        stages: vec![
            stage("Started", l.started),
            stage("Second hour", l.hour2),
            stage("Network sign-up", l.supplemental),
            stage("Badge earned", l.badge),
        ],
        note: None,
    });

    let dl = sqlx::query!(
        r#"SELECT
            COUNT(*) AS "started!: i64",
            COUNT(tutorial_completed_at) AS "tutorial!: i64",
            COUNT(badge_granted_at) AS "badge!: i64"
           FROM data_literacy_progress"#
    )
    .fetch_one(db())
    .await?;
    funnels.push(WorkflowFunnel {
        name: "Building Data Literacy".to_string(),
        stages: vec![
            stage("Started", dl.started),
            stage("Tutorial", dl.tutorial),
            stage("Badge earned", dl.badge),
        ],
        note: None,
    });

    let he = sqlx::query!(
        r#"SELECT
            COUNT(*) AS "started!: i64",
            COUNT(tutorial_completed_at) AS "tutorial!: i64",
            COUNT(list_completed_at) AS "list!: i64",
            COUNT(explore_completed_at) AS "explore!: i64",
            COUNT(badge_granted_at) AS "badge!: i64"
           FROM higher_ed_progress"#
    )
    .fetch_one(db())
    .await?;
    funnels.push(WorkflowFunnel {
        name: "Teaching in Higher Education".to_string(),
        stages: vec![
            stage("Started", he.started),
            stage("Tutorial", he.tutorial),
            stage("Made a list", he.list),
            stage("Explored", he.explore),
            stage("Badge earned", he.badge),
        ],
        note: None,
    });

    let de = sqlx::query!(
        r#"SELECT
            COUNT(*) AS "started!: i64",
            COUNT(tutorial_completed_at) AS "tutorial!: i64",
            COUNT(badge_granted_at) AS "badge!: i64"
           FROM data_ethics_progress"#
    )
    .fetch_one(db())
    .await?;
    funnels.push(WorkflowFunnel {
        name: "Data Ethics for Practitioners".to_string(),
        stages: vec![
            stage("Started", de.started),
            stage("Tutorial", de.tutorial),
            stage("Badge earned", de.badge),
        ],
        note: None,
    });

    let am = sqlx::query!(
        r#"SELECT
            COUNT(*) AS "started!: i64",
            COUNT(tutorial_completed_at) AS "tutorial!: i64",
            COUNT(badge_granted_at) AS "badge!: i64"
           FROM ambassador_progress"#
    )
    .fetch_one(db())
    .await?;
    funnels.push(WorkflowFunnel {
        name: "SciStarter Ambassador".to_string(),
        stages: vec![
            stage("Started", am.started),
            stage("Tutorial", am.tutorial),
            stage("Badge earned", am.badge),
        ],
        note: None,
    });

    Ok(WorkflowFunnelResponse { funnels })
}
