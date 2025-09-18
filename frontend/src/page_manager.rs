use anyhow::Error;
use common::{
    message::user::{PageManagerSyncRequest, PageManagerSyncResponse, PageManagerUpdateRequest, PageManagerUpdateResponse},
    service::request_with_retry,
    shared::db,
};
use ulid::Ulid;

/// Sync all page manager flags based on current project/event page editors
/// This queries the frontend database for all users who are page editors and updates their status
pub async fn sync_all_page_managers() -> Result<PageManagerSyncResponse, Error> {
    // Query frontend database for all user IDs that are page editors
    let page_manager_user_ids = get_all_page_manager_user_ids().await?;
    
    let sync_request = PageManagerSyncRequest {
        user_ids: page_manager_user_ids,
    };
    
    // Send bulk sync request to user service
    request_with_retry("user.page_manager.sync", sync_request).await
}

/// Update page manager status for a specific user
pub async fn update_user_page_manager_status(user_id: Ulid) -> Result<PageManagerUpdateResponse, Error> {
    // Check if this user is a page editor for any projects or events
    let is_page_manager = user_is_page_editor(user_id).await?;

    let update_request = PageManagerUpdateRequest {
        user_id,
        is_page_manager,
    };

    // Send individual update request to user service with retry
    request_with_retry("user.page_manager.update", update_request).await
}

/// Query the frontend database to get all user IDs that should be page managers
/// A user is a page manager if they are in project_page_editors or event_page_editors tables
async fn get_all_page_manager_user_ids() -> Result<Vec<Ulid>, Error> {
    // Query for all unique user IDs that are page editors
    let rows = sqlx::query!(
        r#"
        SELECT DISTINCT user_id 
        FROM (
            SELECT user_id FROM project_page_editors WHERE user_id IS NOT NULL
            UNION
            SELECT user_id FROM event_page_editors WHERE user_id IS NOT NULL
        )
        "#
    )
    .fetch_all(db())
    .await?;
    
    let mut user_ids = Vec::new();
    for row in rows {
        if let Ok(user_id) = row.user_id.parse::<Ulid>() {
            user_ids.push(user_id);
        }
    }
    
    Ok(user_ids)
}

/// Check if a specific user is a page editor for any projects or events
async fn user_is_page_editor(user_id: Ulid) -> Result<bool, Error> {
    let user_id_str = user_id.to_string();
    
    let row = sqlx::query!(
        r#"
        SELECT COUNT(*) as count
        FROM (
            SELECT user_id FROM project_page_editors WHERE user_id = ?
            UNION ALL
            SELECT user_id FROM event_page_editors WHERE user_id = ?
        )
        "#,
        user_id_str,
        user_id_str
    )
    .fetch_one(db())
    .await?;
    
    Ok(row.count > 0)
}

/// Trigger page manager sync after project/event page editor changes
/// This should be called after any operation that adds/removes page editors
pub async fn trigger_page_manager_sync_for_users(user_ids: &[Ulid]) -> Result<(), Error> {
    // Update status for all affected users
    for &user_id in user_ids {
        update_user_page_manager_status(user_id).await?;
    }

    Ok(())
}

/// Convenience function for single user updates
pub async fn trigger_page_manager_sync_for_user(user_id: Ulid) -> Result<(), Error> {
    trigger_page_manager_sync_for_users(&[user_id]).await
}

/// Full sync of all page manager statuses - use for periodic maintenance or after bulk operations
pub async fn full_page_manager_sync() -> Result<u64, Error> {
    let response = sync_all_page_managers().await?;
    println!("Updated {} user page manager statuses", response.updated_count);
    Ok(response.updated_count)
}

// Example integration patterns for project/event management:

/// Example: Call this after adding a user to project_page_editors
pub async fn after_project_editor_added(user_id: Ulid, _project_id: &str) -> Result<(), Error> {
    // After: INSERT INTO project_page_editors (user_id, project_id) VALUES (?, ?)
    trigger_page_manager_sync_for_user(user_id).await
}

/// Example: Call this after removing a user from project_page_editors
pub async fn after_project_editor_removed(user_id: Ulid, _project_id: &str) -> Result<(), Error> {
    // After: DELETE FROM project_page_editors WHERE user_id = ? AND project_id = ?
    trigger_page_manager_sync_for_user(user_id).await
}

/// Example: Call this after adding a user to event_page_editors
pub async fn after_event_editor_added(user_id: Ulid, _event_id: &str) -> Result<(), Error> {
    // After: INSERT INTO event_page_editors (user_id, event_id) VALUES (?, ?)
    trigger_page_manager_sync_for_user(user_id).await
}

/// Example: Call this after removing a user from event_page_editors
pub async fn after_event_editor_removed(user_id: Ulid, _event_id: &str) -> Result<(), Error> {
    // After: DELETE FROM event_page_editors WHERE user_id = ? AND event_id = ?
    trigger_page_manager_sync_for_user(user_id).await
}

/// Example: Call this after a new project is created and page editors are assigned
pub async fn after_project_added(page_creator_user_id: Option<Ulid>, editor_user_ids: &[Ulid]) -> Result<(), Error> {
    let mut all_affected_users = Vec::new();
    
    // Add page creator if specified
    if let Some(creator_id) = page_creator_user_id {
        all_affected_users.push(creator_id);
    }
    
    // Add all editor user IDs
    all_affected_users.extend_from_slice(editor_user_ids);
    
    // Remove duplicates
    all_affected_users.sort();
    all_affected_users.dedup();
    
    // Update page manager status for all affected users
    trigger_page_manager_sync_for_users(&all_affected_users).await
}

/// Example: Call this after a new event is created and page editors are assigned
pub async fn after_event_added(page_creator_user_id: Option<Ulid>, editor_user_ids: &[Ulid]) -> Result<(), Error> {
    let mut all_affected_users = Vec::new();
    
    // Add page creator if specified
    if let Some(creator_id) = page_creator_user_id {
        all_affected_users.push(creator_id);
    }
    
    // Add all editor user IDs
    all_affected_users.extend_from_slice(editor_user_ids);
    
    // Remove duplicates
    all_affected_users.sort();
    all_affected_users.dedup();
    
    // Update page manager status for all affected users
    trigger_page_manager_sync_for_users(&all_affected_users).await
}

/// Example: Call this after bulk project/event sync operations
pub async fn after_bulk_sync(affected_user_ids: Vec<Ulid>) -> Result<(), Error> {
    // After bulk operations that might have changed project_page_editors or event_page_editors
    if affected_user_ids.len() > 10 {
        // For large numbers of users, do a full sync
        full_page_manager_sync().await?;
    } else {
        // For smaller numbers, update individually
        trigger_page_manager_sync_for_users(&affected_user_ids).await?;
    }
    Ok(())
}