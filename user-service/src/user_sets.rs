use crate::db;
use anyhow::{Result, anyhow};
use async_nats::service::Request;
use common::message::log::{AffiliateParticipantsRequest, AffiliateParticipantsResponse};
use common::message::user::{
    CriterionOperation, UserSetAddCriterionRequest, UserSetCreateRequest, UserSetCriterion,
    UserSetCriterionResponse, UserSetCriterionWithId, UserSetDeleteRequest,
    UserSetDetailResponse, UserSetGetRequest, UserSetGetResponse, UserSetInfo,
    UserSetListResponse, UserSetReleaseResultsRequest, UserSetReleaseResultsResponse,
    UserSetRemoveCriterionRequest, UserSetResponse, UserSetUpdateRequest,
};
use common::service::{decode_structured_message, request};
use ulid::Ulid;

/// List all user sets
pub async fn list(_req: &Request) -> Result<UserSetListResponse> {
    let rows =
        sqlx::query!("SELECT slug, name, created_at FROM user_sets ORDER BY created_at DESC")
            .fetch_all(db())
            .await?;

    let user_sets = rows
        .into_iter()
        .map(|row| UserSetInfo {
            slug: row.slug,
            name: row.name,
            created_at: row.created_at,
        })
        .collect();

    Ok(UserSetListResponse { user_sets })
}

/// Get user IDs for a specific user set
/// Uses SQL-based set operations with progressive intersection to handle large sets efficiently
/// If evaluation_id is provided, uses existing results; otherwise computes new result set
pub async fn get(req: &Request) -> Result<UserSetGetResponse> {
    let request: UserSetGetRequest = decode_structured_message(req.message.clone())?;

    // Get the user set info
    let set_row = sqlx::query!(
        "SELECT slug, name FROM user_sets WHERE slug = ?",
        request.slug
    )
    .fetch_one(db())
    .await?;

    // Determine if we should use an existing result set or compute a new one
    let evaluation_id = if let Some(existing_id) = request.evaluation_id {
        // Check if the result set exists
        let exists = sqlx::query!(
            r#"SELECT COUNT(*) as "count!" FROM user_set_results WHERE evaluation_id = ? LIMIT 1"#,
            existing_id
        )
        .fetch_one(db())
        .await?
        .count > 0;

        if !exists {
            return Err(anyhow!("Result set not found or has been released"));
        }

        existing_id
    } else {
        // Compute new result set
        let new_evaluation_id = Ulid::new().to_string();

        // Get all criteria for this set
        let criteria_rows = sqlx::query!(
            "SELECT criterion_type, criterion_data, operation FROM user_set_criteria WHERE user_set_slug = ? ORDER BY id",
            request.slug
        )
        .fetch_all(db())
        .await?;

        let mut has_intersect = false;

        // Process each criterion
        for (index, row) in criteria_rows.iter().enumerate() {
            let criterion: UserSetCriterion =
                flexbuffers::from_slice(&row.criterion_data).map_err(|e| {
                    anyhow!(
                        "Failed to deserialize criterion data for type {}: {}",
                        row.criterion_type,
                        e
                    )
                })?;

            let operation = row.operation.parse::<CriterionOperation>().unwrap_or_default();

            match operation {
                CriterionOperation::Union => {
                    // For union criteria, just insert users into the union set
                    evaluate_criterion(&criterion, &new_evaluation_id, index as i64, "union").await?;
                }
                CriterionOperation::Intersect => {
                    if !has_intersect {
                        // First intersect criterion - initialize the intersect set
                        evaluate_criterion(&criterion, &new_evaluation_id, index as i64, "intersect").await?;
                        has_intersect = true;
                    } else {
                        // Subsequent intersect criteria - progressively narrow the intersect set
                        // Create a temp table for this criterion's results
                        let temp_eval_id = format!("{}_temp_{}", new_evaluation_id, index);
                        evaluate_criterion(&criterion, &temp_eval_id, index as i64, "intersect").await?;

                        // Delete from intersect set any users NOT in the new criterion
                        sqlx::query!(
                            r#"DELETE FROM user_set_evaluations
                               WHERE evaluation_id = ?
                                 AND operation = 'intersect'
                                 AND user_id NOT IN (
                                   SELECT user_id FROM user_set_evaluations
                                   WHERE evaluation_id = ?
                                 )"#,
                            new_evaluation_id,
                            temp_eval_id
                        )
                        .execute(db())
                        .await?;

                        // Clean up temp results
                        sqlx::query!(
                            "DELETE FROM user_set_evaluations WHERE evaluation_id = ?",
                            temp_eval_id
                        )
                        .execute(db())
                        .await?;
                    }
                }
            }
        }

        // Compute the final result set and store it in the results table
        // This is done ONCE, then all pagination queries use the results table
        let created_at = common::timestring::TimeString::now();

        if has_intersect {
            // Insert intersection of union and intersect sets into results
            sqlx::query!(
                r#"INSERT INTO user_set_results (evaluation_id, user_id, created_at)
                   SELECT DISTINCT ?, user_id, ?
                   FROM user_set_evaluations
                   WHERE evaluation_id = ?
                     AND operation = 'union'
                     AND user_id IN (
                       SELECT user_id FROM user_set_evaluations
                       WHERE evaluation_id = ? AND operation = 'intersect'
                     )"#,
                new_evaluation_id,
                created_at,
                new_evaluation_id,
                new_evaluation_id
            )
            .execute(db())
            .await?;
        } else {
            // Insert all union users into results
            sqlx::query!(
                r#"INSERT INTO user_set_results (evaluation_id, user_id, created_at)
                   SELECT DISTINCT ?, user_id, ?
                   FROM user_set_evaluations
                   WHERE evaluation_id = ? AND operation = 'union'"#,
                new_evaluation_id,
                created_at,
                new_evaluation_id
            )
            .execute(db())
            .await?;
        }

        // Clean up evaluations table immediately
        sqlx::query!(
            "DELETE FROM user_set_evaluations WHERE evaluation_id = ?",
            new_evaluation_id
        )
        .execute(db())
        .await?;

        new_evaluation_id
    };

    // Get total count from results table
    let total_count = sqlx::query!(
        r#"SELECT COUNT(*) as "count!" FROM user_set_results WHERE evaluation_id = ?"#,
        evaluation_id
    )
    .fetch_one(db())
    .await?
    .count as u64;

    // Compute pagination
    let (limit, offset) = if let (Some(page), Some(limit)) = (request.page, request.limit) {
        (limit as i64, (page * limit) as i64)
    } else {
        // No pagination - return all results
        (total_count as i64 + 1, 0)
    };

    // Fetch paginated results from results table (fast indexed query)
    let user_ids: Vec<Ulid> = sqlx::query!(
        r#"SELECT user_id FROM user_set_results
           WHERE evaluation_id = ?
           LIMIT ? OFFSET ?"#,
        evaluation_id,
        limit,
        offset
    )
    .fetch_all(db())
    .await?
    .into_iter()
    .filter_map(|row| row.user_id.parse::<Ulid>().ok())
    .collect();

    let has_more = (offset as u64 + user_ids.len() as u64) < total_count;

    Ok(UserSetGetResponse {
        slug: set_row.slug,
        name: set_row.name,
        user_ids,
        total_count,
        has_more,
        evaluation_id,
    })
}

/// Evaluate a single criterion and insert matching user IDs into the evaluations table
/// This processes results in chunks to avoid loading large sets into memory
async fn evaluate_criterion(
    criterion: &UserSetCriterion,
    evaluation_id: &str,
    criterion_index: i64,
    operation: &str,
) -> Result<()> {
    let created_at = common::timestring::TimeString::now();

    match criterion {
        UserSetCriterion::HasTag { tag } => {
            // Insert directly from query result - no intermediate collection
            sqlx::query!(
                r#"INSERT INTO user_set_evaluations (evaluation_id, criterion_index, operation, user_id, created_at)
                   SELECT ?, ?, ?, user_id, ?
                   FROM user_tags
                   WHERE tag = ?"#,
                evaluation_id,
                criterion_index,
                operation,
                created_at,
                tag
            )
            .execute(db())
            .await?;

            Ok(())
        }

        UserSetCriterion::HasPointInRegion { geojson } => {
            // Use SpatiaLite to check if user's point is within the GeoJSON region
            sqlx::query!(
                r#"INSERT INTO user_set_evaluations (evaluation_id, criterion_index, operation, user_id, created_at)
                   SELECT ?, ?, ?, user_id, ?
                   FROM users
                   WHERE point IS NOT NULL
                     AND ST_Within(point, GeomFromGeoJSON(?)) = 1"#,
                evaluation_id,
                criterion_index,
                operation,
                created_at,
                geojson
            )
            .execute(db())
            .await?;

            Ok(())
        }

        UserSetCriterion::AffiliateParticipation {
            affiliate_id,
            start,
            end,
        } => {
            // Query the log service for users who participated in this affiliate
            let req = AffiliateParticipantsRequest {
                affiliate_id: *affiliate_id,
                start: start.clone(),
                end: end.clone(),
            };

            let response: AffiliateParticipantsResponse =
                request("log.affiliate_participants", &req).await?;

            // Insert the results in batches to avoid huge single inserts
            const BATCH_SIZE: usize = 1000;
            for chunk in response.user_ids.chunks(BATCH_SIZE) {
                for user_id in chunk {
                    let user_id_str = user_id.to_string();
                    sqlx::query!(
                        r#"INSERT INTO user_set_evaluations (evaluation_id, criterion_index, operation, user_id, created_at)
                           VALUES (?, ?, ?, ?, ?)"#,
                        evaluation_id,
                        criterion_index,
                        operation,
                        user_id_str,
                        created_at
                    )
                    .execute(db())
                    .await?;
                }
            }

            Ok(())
        }

        UserSetCriterion::IsPageManager => {
            sqlx::query!(
                r#"INSERT INTO user_set_evaluations (evaluation_id, criterion_index, operation, user_id, created_at)
                   SELECT ?, ?, ?, user_id, ?
                   FROM users
                   WHERE is_page_manager = 1"#,
                evaluation_id,
                criterion_index,
                operation,
                created_at
            )
            .execute(db())
            .await?;

            Ok(())
        }

        UserSetCriterion::IsStaff => {
            sqlx::query!(
                r#"INSERT INTO user_set_evaluations (evaluation_id, criterion_index, operation, user_id, created_at)
                   SELECT ?, ?, ?, user_id, ?
                   FROM users
                   WHERE is_staff = 1 OR is_superuser = 1"#,
                evaluation_id,
                criterion_index,
                operation,
                created_at
            )
            .execute(db())
            .await?;

            Ok(())
        }

        UserSetCriterion::IsVerified => {
            sqlx::query!(
                r#"INSERT INTO user_set_evaluations (evaluation_id, criterion_index, operation, user_id, created_at)
                   SELECT ?, ?, ?, u.user_id, ?
                   FROM users u
                   JOIN emails e ON u.primary_email = e.email
                   WHERE e.verified = 1"#,
                evaluation_id,
                criterion_index,
                operation,
                created_at
            )
            .execute(db())
            .await?;

            Ok(())
        }

        UserSetCriterion::ExplicitList { user_ids } => {
            // Insert the explicit list in batches
            const BATCH_SIZE: usize = 1000;
            for chunk in user_ids.chunks(BATCH_SIZE) {
                for user_id in chunk {
                    let user_id_str = user_id.to_string();
                    sqlx::query!(
                        r#"INSERT INTO user_set_evaluations (evaluation_id, criterion_index, operation, user_id, created_at)
                           VALUES (?, ?, ?, ?, ?)"#,
                        evaluation_id,
                        criterion_index,
                        operation,
                        user_id_str,
                        created_at
                    )
                    .execute(db())
                    .await?;
                }
            }

            Ok(())
        }
    }
}

/// Create a new user set
pub async fn create(req: &Request) -> Result<UserSetResponse> {
    let request: UserSetCreateRequest = decode_structured_message(req.message.clone())?;

    // Insert the user set
    sqlx::query!(
        "INSERT INTO user_sets (slug, name) VALUES (?, ?)",
        request.slug,
        request.name
    )
    .execute(db())
    .await?;

    Ok(UserSetResponse {
        success: true,
        message: format!("User set '{}' created successfully", request.slug),
    })
}

/// Update an existing user set
pub async fn update(req: &Request) -> Result<UserSetResponse> {
    let request: UserSetUpdateRequest = decode_structured_message(req.message.clone())?;

    // Update the user set
    sqlx::query!(
        "UPDATE user_sets SET name = ? WHERE slug = ?",
        request.name,
        request.slug
    )
    .execute(db())
    .await?;

    Ok(UserSetResponse {
        success: true,
        message: format!("User set '{}' updated successfully", request.slug),
    })
}

/// Delete a user set
pub async fn delete(req: &Request) -> Result<UserSetResponse> {
    let request: UserSetDeleteRequest = decode_structured_message(req.message.clone())?;

    // Delete the user set (criteria will be cascade deleted)
    sqlx::query!("DELETE FROM user_sets WHERE slug = ?", request.slug)
        .execute(db())
        .await?;

    Ok(UserSetResponse {
        success: true,
        message: format!("User set '{}' deleted successfully", request.slug),
    })
}

/// Get user set details with criteria
pub async fn get_detail(req: &Request) -> Result<UserSetDetailResponse> {
    let request: UserSetGetRequest = decode_structured_message(req.message.clone())?;

    // Get the user set info
    let set_row = sqlx::query!(
        "SELECT slug, name FROM user_sets WHERE slug = ?",
        request.slug
    )
    .fetch_one(db())
    .await?;

    // Get all criteria for this set
    let criteria_rows = sqlx::query!(
        r#"SELECT id as "id!", criterion_type, criterion_data, operation FROM user_set_criteria WHERE user_set_slug = ? ORDER BY id"#,
        request.slug
    )
    .fetch_all(db())
    .await?;

    let criteria = criteria_rows
        .into_iter()
        .filter_map(|row| {
            let criterion: UserSetCriterion = flexbuffers::from_slice(&row.criterion_data).ok()?;
            let operation = row.operation.parse::<CriterionOperation>().unwrap_or_default();
            Some(UserSetCriterionWithId {
                id: row.id,
                criterion,
                operation,
            })
        })
        .collect();

    Ok(UserSetDetailResponse {
        slug: set_row.slug,
        name: set_row.name,
        criteria,
    })
}

/// Add a criterion to a user set
pub async fn add_criterion(req: &Request) -> Result<UserSetCriterionResponse> {
    let request: UserSetAddCriterionRequest = decode_structured_message(req.message.clone())?;

    // Serialize the criterion using flexbuffers
    let criterion_data = flexbuffers::to_vec(&request.criterion)?;

    // Determine the criterion type as a string for querying
    let criterion_type = match &request.criterion {
        UserSetCriterion::HasTag { .. } => "HasTag",
        UserSetCriterion::HasPointInRegion { .. } => "HasPointInRegion",
        UserSetCriterion::AffiliateParticipation { .. } => "AffiliateParticipation",
        UserSetCriterion::IsPageManager => "IsPageManager",
        UserSetCriterion::IsStaff => "IsStaff",
        UserSetCriterion::IsVerified => "IsVerified",
        UserSetCriterion::ExplicitList { .. } => "ExplicitList",
    };

    // Convert operation to string
    let operation = request.operation.as_ref();

    // Insert the criterion
    sqlx::query!(
        "INSERT INTO user_set_criteria (user_set_slug, criterion_type, criterion_data, operation) VALUES (?, ?, ?, ?)",
        request.slug,
        criterion_type,
        criterion_data,
        operation
    )
    .execute(db())
    .await?;

    Ok(UserSetCriterionResponse {
        success: true,
        message: "Criterion added successfully".to_string(),
    })
}

/// Remove a criterion from a user set
pub async fn remove_criterion(req: &Request) -> Result<UserSetCriterionResponse> {
    let request: UserSetRemoveCriterionRequest = decode_structured_message(req.message.clone())?;

    // Delete the criterion
    sqlx::query!(
        "DELETE FROM user_set_criteria WHERE id = ? AND user_set_slug = ?",
        request.criterion_id,
        request.slug
    )
    .execute(db())
    .await?;

    Ok(UserSetCriterionResponse {
        success: true,
        message: "Criterion removed successfully".to_string(),
    })
}

/// Release (delete) a specific result set immediately
/// Call this when you're done paginating through a result set to free up memory
pub async fn release_results(req: &Request) -> Result<UserSetReleaseResultsResponse> {
    let request: UserSetReleaseResultsRequest = decode_structured_message(req.message.clone())?;

    sqlx::query!(
        "DELETE FROM user_set_results WHERE evaluation_id = ?",
        request.evaluation_id
    )
    .execute(db())
    .await?;

    // Also clean up any in-progress evaluations with this ID (shouldn't exist, but just in case)
    sqlx::query!(
        "DELETE FROM user_set_evaluations WHERE evaluation_id = ?",
        request.evaluation_id
    )
    .execute(db())
    .await?;

    Ok(UserSetReleaseResultsResponse { success: true })
}

/// Clean up old result sets
/// Should be called periodically (e.g., daily) to remove result sets older than 1 week
pub async fn cleanup_old_results() -> Result<()> {
    use jiff::{Timestamp, ToSpan};

    // Compute cutoff time (1 week ago)
    let now = Timestamp::now();
    let one_week = 7.days();
    let one_week_ago_ts = now.checked_sub(one_week)?;
    let one_week_ago = common::timestring::TimeString::from(one_week_ago_ts);

    // Clean up old result sets
    let result = sqlx::query!(
        "DELETE FROM user_set_results WHERE created_at < ?",
        one_week_ago
    )
    .execute(db())
    .await?;

    if result.rows_affected() > 0 {
        eprintln!(
            "Cleaned up {} old user set result sets",
            result.rows_affected()
        );
    }

    Ok(())
}
