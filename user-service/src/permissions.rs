use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{
        Permission, PermissionCheckRequest, PermissionCheckResponse, PermissionGrantRequest,
        PermissionListByResourceRequest, PermissionListByResourceResponse, PermissionListRequest,
        PermissionListResponse, PermissionRevokeRequest,
    },
    service::decode_structured_message,
    shared::db,
    timestring::TimeString,
};

pub async fn check_permission(req: &Request) -> Result<PermissionCheckResponse, Error> {
    let req: PermissionCheckRequest = decode_structured_message(req.message.clone())?;

    let result = sqlx::query_scalar!(
        r#"
        SELECT user_id
        FROM permissions
        WHERE user_id = ? AND resource_type = ? AND resource_id = ?
        LIMIT 1
        "#,
        req.user_id,
        req.resource_type,
        req.resource_id
    )
    .fetch_optional(db())
    .await?;

    Ok(PermissionCheckResponse {
        has_permission: result.is_some(),
    })
}

pub async fn grant_permission(req: &Request) -> Result<(), Error> {
    let req: PermissionGrantRequest = decode_structured_message(req.message.clone())?;

    let now = TimeString::now().to_string();

    sqlx::query!(
        r#"
        INSERT OR IGNORE INTO permissions
        (user_id, resource_type, resource_id, permission, granted_by, granted_at)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
        req.user_id,
        req.resource_type,
        req.resource_id,
        req.permission,
        req.granted_by,
        now
    )
    .execute(db())
    .await?;

    Ok(())
}

pub async fn revoke_permission(req: &Request) -> Result<(), Error> {
    let req: PermissionRevokeRequest = decode_structured_message(req.message.clone())?;

    sqlx::query!(
        r#"
        DELETE FROM permissions
        WHERE user_id = ? AND resource_type = ? AND resource_id = ?
        "#,
        req.user_id,
        req.resource_type,
        req.resource_id
    )
    .execute(db())
    .await?;

    Ok(())
}

pub async fn list_permissions(req: &Request) -> Result<PermissionListResponse, Error> {
    let req: PermissionListRequest = decode_structured_message(req.message.clone())?;

    let permissions = if let Some(resource_type) = req.resource_type {
        sqlx::query_as!(
            Permission,
            r#"
            SELECT user_id, resource_type, resource_id, permission, granted_by, granted_at
            FROM permissions
            WHERE user_id = ? AND resource_type = ?
            "#,
            req.user_id,
            resource_type
        )
        .fetch_all(db())
        .await?
    } else {
        sqlx::query_as!(
            Permission,
            r#"
            SELECT user_id, resource_type, resource_id, permission, granted_by, granted_at
            FROM permissions
            WHERE user_id = ?
            "#,
            req.user_id
        )
        .fetch_all(db())
        .await?
    };

    Ok(PermissionListResponse { permissions })
}

pub async fn list_permissions_by_resource(
    req: &Request,
) -> Result<PermissionListByResourceResponse, Error> {
    let req: PermissionListByResourceRequest = decode_structured_message(req.message.clone())?;

    let permissions = sqlx::query_as!(
        Permission,
        r#"
        SELECT user_id, resource_type, resource_id, permission, granted_by, granted_at
        FROM permissions
        WHERE resource_type = ? AND resource_id = ?
        "#,
        req.resource_type,
        req.resource_id
    )
    .fetch_all(db())
    .await?;

    Ok(PermissionListByResourceResponse { permissions })
}
