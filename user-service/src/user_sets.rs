use crate::db;
use anyhow::{Result, anyhow};
use async_nats::service::Request;
use common::message::log::{AffiliateParticipantsRequest, AffiliateParticipantsResponse};
use common::message::user::{
    CriterionOperation, UserSetAddCriterionRequest, UserSetCreateRequest, UserSetCriterion,
    UserSetCriterionResponse, UserSetCriterionWithId, UserSetDeleteRequest,
    UserSetDetailResponse, UserSetGetRequest, UserSetGetResponse, UserSetInfo,
    UserSetListResponse, UserSetRemoveCriterionRequest, UserSetResponse, UserSetUpdateRequest,
};
use common::service::{decode_structured_message, request};
use std::collections::HashSet;
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
pub async fn get(req: &Request) -> Result<UserSetGetResponse> {
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
        "SELECT criterion_type, criterion_data, operation FROM user_set_criteria WHERE user_set_slug = ? ORDER BY id",
        request.slug
    )
    .fetch_all(db())
    .await?;

    // Process criteria with proper union/intersect logic
    let mut union_set: HashSet<Ulid> = HashSet::new();
    let mut intersect_set: Option<HashSet<Ulid>> = None;

    for row in criteria_rows {
        let criterion: UserSetCriterion =
            flexbuffers::from_slice(&row.criterion_data).map_err(|e| {
                anyhow!(
                    "Failed to deserialize criterion data for type {}: {}",
                    row.criterion_type,
                    e
                )
            })?;

        let operation = row.operation.parse::<CriterionOperation>().unwrap_or_default();
        let criterion_users = evaluate_criterion(&criterion).await?;

        match operation {
            CriterionOperation::Union => {
                // Union with the union intermediate set
                union_set.extend(criterion_users);
            }
            CriterionOperation::Intersect => {
                // If intersect_set exists, intersect it with the new criterion
                // Otherwise, initialize it with the criterion's users
                if let Some(ref mut set) = intersect_set {
                    *set = set.intersection(&criterion_users).copied().collect();
                } else {
                    intersect_set = Some(criterion_users);
                }
            }
        }
    }

    // Final result: if intersect_set exists, intersect it with union_set
    let all_users = if let Some(intersect) = intersect_set {
        union_set.intersection(&intersect).copied().collect()
    } else {
        union_set
    };

    Ok(UserSetGetResponse {
        slug: set_row.slug,
        name: set_row.name,
        user_ids: all_users.into_iter().collect(),
    })
}

/// Evaluate a single criterion and return matching user IDs
async fn evaluate_criterion(criterion: &UserSetCriterion) -> Result<HashSet<Ulid>> {
    match criterion {
        UserSetCriterion::HasTag { tag } => {
            let rows = sqlx::query!("SELECT DISTINCT user_id FROM user_tags WHERE tag = ?", tag)
                .fetch_all(db())
                .await?;

            let user_ids = rows
                .into_iter()
                .filter_map(|row| row.user_id.parse::<Ulid>().ok())
                .collect();

            Ok(user_ids)
        }

        UserSetCriterion::HasPointInRegion { geojson } => {
            // Use SpatiaLite to check if user's point is within the GeoJSON region
            let rows = sqlx::query!(
                r#"
                SELECT user_id
                FROM users
                WHERE point IS NOT NULL
                  AND ST_Within(point, GeomFromGeoJSON(?)) = 1
                "#,
                geojson
            )
            .fetch_all(db())
            .await?;

            let user_ids = rows
                .into_iter()
                .filter_map(|row| row.user_id.parse::<Ulid>().ok())
                .collect();

            Ok(user_ids)
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

            Ok(response.user_ids.into_iter().collect())
        }

        UserSetCriterion::IsPageManager => {
            let rows = sqlx::query!("SELECT user_id FROM users WHERE is_page_manager = 1")
                .fetch_all(db())
                .await?;

            let user_ids = rows
                .into_iter()
                .filter_map(|row| row.user_id.parse::<Ulid>().ok())
                .collect();

            Ok(user_ids)
        }

        UserSetCriterion::IsStaff => {
            let rows =
                sqlx::query!("SELECT user_id FROM users WHERE is_staff = 1 OR is_superuser = 1")
                    .fetch_all(db())
                    .await?;

            let user_ids = rows
                .into_iter()
                .filter_map(|row| row.user_id.parse::<Ulid>().ok())
                .collect();

            Ok(user_ids)
        }

        UserSetCriterion::ExplicitList { user_ids } => {
            // Return the explicit list of user IDs
            Ok(user_ids.iter().copied().collect())
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
