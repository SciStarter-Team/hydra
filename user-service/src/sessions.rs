use anyhow::Error;
use async_nats::service::Request;
use common::{message::user::Session, shared::db};

/// Store session data
///
/// Does not reply, so the requestor should not wait for a
/// response. In other words, use publish rather than request when
/// communicating with this endpoint.
pub async fn store(req: Request) -> Result<(), Error> {
    let session: Session = flexbuffers::from_slice(&req.message.payload)?;

    sqlx::query!(
        r#"insert into "sessions" ("session_id", "expires", "data")
           values (?, ?, ?)
           on conflict ("session_id") do
           update set "expires" = excluded."expires", "data" = excluded."data""#,
        session.session_id,
        session.expires,
        session.data
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Fetch session data
pub async fn retrieve(req: Request) -> Result<(), Error> {
    let session_id: String = flexbuffers::from_slice(&req.message.payload)?;

    let session = sqlx::query_as!(
        Session,
        r#"select "session_id", "expires", "data" from "sessions" where "session_id" = ?"#,
        session_id
    )
    .fetch_optional(db())
    .await?;

    let payload = flexbuffers::to_vec(session)?;
    req.respond(Ok(payload.into())).await?;

    Ok(())
}

/// Delete session data
///
/// Does not reply, so the requestor should not wait for a
/// response. In other words, use publish rather than request when
/// communicating with this endpoint.
pub async fn delete(req: Request) -> Result<(), Error> {
    let session_id: String = flexbuffers::from_slice(&req.message.payload)?;

    sqlx::query!(
        r#"delete from "sessions" where "session_id" = ?"#,
        session_id
    )
    .execute(db())
    .await?;

    Ok(())
}
