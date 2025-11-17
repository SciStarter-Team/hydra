// Workflow progress tracking for Articulate training programs

use async_nats::service::Request;
use common::{service::{decode_structured_message, send, trigger_email, TriggerEvent}, shared::db};
use common::message::user::{WorkflowProgressRequest, WorkflowProgressResponse, WorkflowType, WorkflowStatusRequest, WorkflowStatusResponse, UserAndBadge};
use common::badge::Badge;
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
        WorkflowType::Ideal => {
            update_ideal_progress(&request.user_id, &request.step, &now).await?
        }
        WorkflowType::Ambassador => {
            update_ambassador_progress(&request.user_id, &request.step, &now).await?
        }
    };

    Ok(WorkflowProgressResponse {
        success: true,
        badge_granted,
    })
}

/// Update Foundations of Citizen Science progress
async fn update_foundations_progress(user_id: &str, step: &str, now: &str) -> Result<bool, anyhow::Error> {
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
    let project_count = [row.has_squirrel.unwrap_or(false), row.has_stall.unwrap_or(false), row.has_debris.unwrap_or(false)]
        .iter()
        .filter(|&&x| x)
        .count();

    if row.has_hour1.unwrap_or(false) && project_count >= 2 {
        let now = jiff::Zoned::now().to_string();
        sqlx::query!(
            "UPDATE foundations_progress SET badge_granted_at = ? WHERE user_id = ?",
            now, user_id
        )
        .execute(db())
        .await?;

        // Add user tag
        send("user.tags.add", (user_id.to_string(), "foundations-badge".to_string())).await?;

        // Grant badge
        let user_ulid = Ulid::from_string(user_id)?;
        send("user.badges.grant", UserAndBadge {
            user_id: user_ulid,
            badge: Badge::FoundationsOfCitizenScience,
        }).await?;

        // Trigger email notification
        trigger_email(TriggerEvent::BadgeFoundations, user_ulid, None).await?;

        return Ok(true);
    }

    Ok(false)
}

/// Update Libraries as Community Hubs progress
async fn update_libraries_hubs_progress(user_id: &str, step: &str, now: &str) -> Result<bool, anyhow::Error> {
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

    if row.has_hour2.unwrap_or(false) && row.has_supplemental.unwrap_or(false) {
        let now = jiff::Zoned::now().to_string();
        sqlx::query!(
            "UPDATE libraries_hubs_progress SET badge_granted_at = ? WHERE user_id = ?",
            now, user_id
        )
        .execute(db())
        .await?;

        // Add user tag
        send("user.tags.add", (user_id.to_string(), "libraries-hubs-badge".to_string())).await?;

        // Grant badge
        let user_ulid = Ulid::from_string(user_id)?;
        send("user.badges.grant", UserAndBadge {
            user_id: user_ulid,
            badge: Badge::LibrariesAsHubsForCitizenScience,
        }).await?;

        // Trigger email notification
        trigger_email(TriggerEvent::BadgeLibrariesHubs, user_ulid, None).await?;

        return Ok(true);
    }

    Ok(false)
}

/// Update Data Literacy progress (single step - grants badge immediately)
async fn update_data_literacy_progress(user_id: &str, step: &str, now: &str) -> Result<bool, anyhow::Error> {
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
        send("user.tags.add", (user_id.to_string(), "data-literacy-badge".to_string())).await?;

        // Grant badge
        let user_ulid = Ulid::from_string(user_id)?;
        send("user.badges.grant", UserAndBadge {
            user_id: user_ulid,
            badge: Badge::BuildingDataLiteracyThroughCommunityAndCitizenScience,
        }).await?;

        // Trigger email notification
        trigger_email(TriggerEvent::BadgeDataLiteracy, user_ulid, None).await?;

        Ok(true)
    } else {
        Ok(false)
    }
}

/// Update Higher Ed progress
async fn update_higher_ed_progress(user_id: &str, step: &str, now: &str) -> Result<bool, anyhow::Error> {
    if step != "tutorial" {
        return Ok(false);
    }

    let result = sqlx::query!(
        "INSERT OR IGNORE INTO higher_ed_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
        user_id, now, now
    )
    .execute(db())
    .await?;

    // Only grant badge if this is a new completion
    if result.rows_affected() > 0 {
        // Add user tag
        send("user.tags.add", (user_id.to_string(), "higher-ed-badge".to_string())).await?;

        // Grant badge
        let user_ulid = Ulid::from_string(user_id)?;
        send("user.badges.grant", UserAndBadge {
            user_id: user_ulid,
            badge: Badge::TeachingInHigherEducationWithCitizenScience,
        }).await?;

        // Trigger email notification
        trigger_email(TriggerEvent::BadgeHigherEd, user_ulid, None).await?;

        Ok(true)
    } else {
        Ok(false)
    }
}

/// Update Data Ethics progress (single step - grants badge immediately)
async fn update_data_ethics_progress(user_id: &str, step: &str, now: &str) -> Result<bool, anyhow::Error> {
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
        send("user.tags.add", (user_id.to_string(), "data-ethics-badge".to_string())).await?;

        // Grant badge
        let user_ulid = Ulid::from_string(user_id)?;
        send("user.badges.grant", UserAndBadge {
            user_id: user_ulid,
            badge: Badge::DataEthicsForPractitioners,
        }).await?;

        // Trigger email notification
        trigger_email(TriggerEvent::BadgeDataEthics, user_ulid, None).await?;

        Ok(true)
    } else {
        Ok(false)
    }
}

/// Update IDEAL progress (single step - grants badge immediately)
async fn update_ideal_progress(user_id: &str, step: &str, now: &str) -> Result<bool, anyhow::Error> {
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
        send("user.tags.add", (user_id.to_string(), "ideal-badge".to_string())).await?;

        // Grant badge
        let user_ulid = Ulid::from_string(user_id)?;
        send("user.badges.grant", UserAndBadge {
            user_id: user_ulid,
            badge: Badge::IdealParticipatoryScience,
        }).await?;

        // Trigger email notification
        trigger_email(TriggerEvent::BadgeIdeal, user_ulid, None).await?;

        Ok(true)
    } else {
        Ok(false)
    }
}

/// Update Ambassador progress
async fn update_ambassador_progress(user_id: &str, step: &str, now: &str) -> Result<bool, anyhow::Error> {
    if step != "tutorial" {
        return Ok(false);
    }

    let result = sqlx::query!(
        "INSERT OR IGNORE INTO ambassador_progress (user_id, tutorial_completed_at, badge_granted_at) VALUES (?, ?, ?)",
        user_id, now, now
    )
    .execute(db())
    .await?;

    // Only grant badge if this is a new completion
    if result.rows_affected() > 0 {
        // Add user tag
        send("user.tags.add", (user_id.to_string(), "ambassador-badge".to_string())).await?;

        // Grant badge
        let user_ulid = Ulid::from_string(user_id)?;
        send("user.badges.grant", UserAndBadge {
            user_id: user_ulid,
            badge: Badge::SciStarterAmbassador,
        }).await?;

        // Trigger email notification
        trigger_email(TriggerEvent::BadgeAmbassador, user_ulid, None).await?;

        Ok(true)
    } else {
        Ok(false)
    }
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
        "SELECT tutorial_completed_at, badge_granted_at FROM higher_ed_progress WHERE user_id = ?",
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
