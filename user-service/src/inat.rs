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
    }

    let rows = sqlx::query_as!(
        Row,
        r#"SELECT user_id, inat_username, inat_last_update
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
            })
        })
        .collect();

    Ok(users)
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
