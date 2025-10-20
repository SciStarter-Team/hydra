use crate::db;
use anyhow::Result;
use async_nats::service::Request;
use common::message::user::{
    AddressSetAddMemberRequest, AddressSetCreateRequest, AddressSetDeleteRequest,
    AddressSetGetRequest, AddressSetGetResponse, AddressSetInfo, AddressSetListResponse,
    AddressSetMemberResponse, AddressSetRemoveMemberRequest, AddressSetResponse,
    AddressSetUpdateRequest,
};
use common::service::decode_structured_message;

/// List all address sets
pub async fn list(_req: &Request) -> Result<AddressSetListResponse> {
    let rows = sqlx::query!(
        "SELECT slug, name, created_at FROM address_sets ORDER BY created_at DESC"
    )
    .fetch_all(db())
    .await?;

    let address_sets = rows
        .into_iter()
        .map(|row| AddressSetInfo {
            slug: row.slug,
            name: row.name,
            created_at: row.created_at,
        })
        .collect();

    Ok(AddressSetListResponse { address_sets })
}

/// Get addresses for a specific address set
/// Supports pagination to handle large address sets efficiently
pub async fn get(req: &Request) -> Result<AddressSetGetResponse> {
    let request: AddressSetGetRequest = decode_structured_message(req.message.clone())?;

    // Get the address set info
    let set_row = sqlx::query!(
        "SELECT slug, name FROM address_sets WHERE slug = ?",
        request.slug
    )
    .fetch_one(db())
    .await?;

    // Get total count
    let total_count = sqlx::query!(
        r#"SELECT COUNT(*) as "count!" FROM address_set_members WHERE address_set_slug = ?"#,
        request.slug
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

    // Get paginated email addresses
    let address_rows = sqlx::query!(
        "SELECT email_address FROM address_set_members WHERE address_set_slug = ? ORDER BY email_address LIMIT ? OFFSET ?",
        request.slug,
        limit,
        offset
    )
    .fetch_all(db())
    .await?;

    let addresses: Vec<String> = address_rows
        .into_iter()
        .map(|row| row.email_address)
        .collect();

    let has_more = (offset as u64 + addresses.len() as u64) < total_count;

    Ok(AddressSetGetResponse {
        slug: set_row.slug,
        name: set_row.name,
        addresses,
        total_count,
        has_more,
    })
}

/// Create a new address set
pub async fn create(req: &Request) -> Result<AddressSetResponse> {
    let request: AddressSetCreateRequest = decode_structured_message(req.message.clone())?;

    // Insert the address set
    sqlx::query!(
        "INSERT INTO address_sets (slug, name) VALUES (?, ?)",
        request.slug,
        request.name
    )
    .execute(db())
    .await?;

    Ok(AddressSetResponse {
        success: true,
        message: format!("Address set '{}' created successfully", request.slug),
    })
}

/// Update an existing address set
pub async fn update(req: &Request) -> Result<AddressSetResponse> {
    let request: AddressSetUpdateRequest = decode_structured_message(req.message.clone())?;

    // Update the address set
    sqlx::query!(
        "UPDATE address_sets SET name = ? WHERE slug = ?",
        request.name,
        request.slug
    )
    .execute(db())
    .await?;

    Ok(AddressSetResponse {
        success: true,
        message: format!("Address set '{}' updated successfully", request.slug),
    })
}

/// Delete an address set
pub async fn delete(req: &Request) -> Result<AddressSetResponse> {
    let request: AddressSetDeleteRequest = decode_structured_message(req.message.clone())?;

    // Delete the address set (members will be cascade deleted)
    sqlx::query!("DELETE FROM address_sets WHERE slug = ?", request.slug)
        .execute(db())
        .await?;

    Ok(AddressSetResponse {
        success: true,
        message: format!("Address set '{}' deleted successfully", request.slug),
    })
}

/// Add a member to an address set
pub async fn add_member(req: &Request) -> Result<AddressSetMemberResponse> {
    let request: AddressSetAddMemberRequest = decode_structured_message(req.message.clone())?;

    // Insert the member (ignore if already exists)
    sqlx::query!(
        "INSERT INTO address_set_members (address_set_slug, email_address) VALUES (?, ?) ON CONFLICT DO NOTHING",
        request.slug,
        request.email_address
    )
    .execute(db())
    .await?;

    Ok(AddressSetMemberResponse {
        success: true,
        message: format!("Added '{}' to address set", request.email_address),
    })
}

/// Remove a member from an address set
pub async fn remove_member(req: &Request) -> Result<AddressSetMemberResponse> {
    let request: AddressSetRemoveMemberRequest = decode_structured_message(req.message.clone())?;

    // Delete the member
    sqlx::query!(
        "DELETE FROM address_set_members WHERE address_set_slug = ? AND email_address = ?",
        request.slug,
        request.email_address
    )
    .execute(db())
    .await?;

    Ok(AddressSetMemberResponse {
        success: true,
        message: format!("Removed '{}' from address set", request.email_address),
    })
}
