use anyhow::Error;
use async_nats::service::Request;
use common::{message::user::BasicInfo, shared::db};

pub async fn basic_info_by_username(req: &Request) -> Result<Option<BasicInfo>, Error> {
    let username: String = flexbuffers::from_slice(&req.message.payload)?;

    Ok(sqlx::query_as!(
        BasicInfo,
        r#"select
          "user_id",
          "handle",
          "is_staff" as "is_staff: bool",
          "is_superuser" as "is_superuser: bool"
        from "users"
        where "username" = ?"#,
        username,
    )
    .fetch_optional(db())
    .await?)
}

pub async fn basic_info_by_user_id(req: &Request) -> Result<Option<BasicInfo>, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;

    Ok(sqlx::query_as!(
        BasicInfo,
        r#"select
          "user_id",
          "handle",
          "is_staff" as "is_staff: bool",
          "is_superuser" as "is_superuser: bool"
        from "users"
        where "user_id" = ?"#,
        user_id,
    )
    .fetch_optional(db())
    .await?)
}
