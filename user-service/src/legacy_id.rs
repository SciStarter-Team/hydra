use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{UserLegacyIdLookupRequest, UserLegacyIdLookupResponse},
    service::decode_structured_message,
    shared::db,
    ulid::AsUlid,
};

pub async fn lookup(req: &Request) -> Result<UserLegacyIdLookupResponse, Error> {
    let message: UserLegacyIdLookupRequest = decode_structured_message(req.message.clone())?;

    let row = sqlx::query!(
        r#"SELECT "user_id"
        FROM "users"
        WHERE "legacy_v3_id" = ? OR "legacy_v2_id" = ?
        LIMIT 1"#,
        message.legacy_id,
        message.legacy_id,
    )
    .fetch_optional(db())
    .await?;

    Ok(UserLegacyIdLookupResponse {
        user_id: row.map(|r| r.user_id.as_ulid()).transpose()?,
    })
}
