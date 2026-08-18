use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{InatListUsersRequest, InatUpdateLastSync, InatUser},
    shared::db,
    ulid::AsUlid,
};

pub async fn list_users(req: &Request) -> Result<Vec<InatUser>, Error> {
    let input: InatListUsersRequest = flexbuffers::from_slice(&req.message.payload)?;

    struct Row {
        user_id: String,
        inat_username: Option<String>,
        inat_last_update: Option<String>,
        inat_linked_at: Option<String>,
    }

    let rows = sqlx::query_as!(
        Row,
        r#"SELECT user_id, inat_username, inat_last_update, inat_linked_at
           FROM users
           WHERE inat_username != '' AND inat_username IS NOT NULL
           ORDER BY user_id
           LIMIT ? OFFSET ?"#,
        input.limit,
        input.offset,
    )
    .fetch_all(db())
    .await?;

    let users = rows
        .into_iter()
        .filter_map(|row| {
            Some(InatUser {
                user_id: row.user_id.as_ulid().ok()?,
                inat_username: row.inat_username?,
                inat_last_update: row.inat_last_update,
                inat_linked_at: row.inat_linked_at,
            })
        })
        .collect();

    Ok(users)
}

/// Whether the user has an iNaturalist username on their profile. Drives the
/// K-12 training's "Affiliation Tracking Check" step.
pub async fn username_set(req: &Request) -> Result<bool, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;

    Ok(sqlx::query_scalar!(
        r#"SELECT COALESCE(inat_username, '') != '' AS "set!: bool" FROM users WHERE user_id = ?"#,
        user_id
    )
    .fetch_optional(db())
    .await?
    .unwrap_or(false))
}

pub async fn update_last_sync(req: &Request) -> Result<(), Error> {
    let input: InatUpdateLastSync = flexbuffers::from_slice(&req.message.payload)?;

    let user_id_str = input.user_id.to_string();

    sqlx::query!(
        r#"UPDATE users SET inat_last_update = ? WHERE user_id = ?"#,
        input.inat_last_update,
        user_id_str,
    )
    .execute(db())
    .await?;

    Ok(())
}
