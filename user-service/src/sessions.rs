use anyhow::Error;
use async_nats::service::Request;
use common::{message::user::Session, shared::db};

pub(crate) async fn read_session(session_id: impl AsRef<str>) -> Result<Option<Session>, Error> {
    let session_id = session_id.as_ref();

    Ok(sqlx::query_as!(
        Session,
        r#"select "session_id", "expires", "data" from "sessions" where "session_id" = ?"#,
        session_id
    )
    .fetch_optional(db())
    .await?)
}

pub(crate) async fn save_session(session: &Session) -> Result<(), Error> {
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

pub(crate) async fn delete_session(session_id: impl AsRef<str>) -> Result<(), Error> {
    let session_id = session_id.as_ref();

    sqlx::query!(
        r#"delete from "sessions" where "session_id" = ?"#,
        session_id
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Store session data
///
/// Does not reply, so the requestor should not wait for a
/// response. In other words, use publish rather than request when
/// communicating with this endpoint.
pub async fn store(req: &Request) -> Result<(), Error> {
    let session: Session = flexbuffers::from_slice(&req.message.payload)?;

    save_session(&session).await
}

/// Fetch session data
pub async fn retrieve(req: &Request) -> Result<Option<Session>, Error> {
    let session_id: String = flexbuffers::from_slice(&req.message.payload)?;

    Ok(read_session(session_id).await?)
}

/// Delete session data
///
/// Does not reply, so the requestor should not wait for a
/// response. In other words, use publish rather than request when
/// communicating with this endpoint.
pub async fn delete(req: &Request) -> Result<(), Error> {
    let session_id: String = flexbuffers::from_slice(&req.message.payload)?;

    delete_session(session_id).await
}
