use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::dashboard::{LinkToken, LinkTokenAdd, LinkTokenDelete},
    service::decode_structured_message,
    shared::db,
    timestring::TimeString,
};
use rand::Rng;

pub async fn list_by_user(req: &Request) -> Result<Vec<LinkToken>, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;

    let rows = sqlx::query!(
        r#"SELECT "token", "label", "created", "expires"
        FROM "link_tokens"
        WHERE "user_id" = ?
        ORDER BY "created" DESC"#,
        user_id
    )
    .fetch_all(db())
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| LinkToken {
            token: row.token,
            label: row.label,
            created: row.created,
            expires: row.expires,
        })
        .collect())
}

pub async fn add(req: &Request) -> Result<LinkToken, Error> {
    let add: LinkTokenAdd = decode_structured_message(req.message.clone())?;
    let user_id_str = add.user_id.to_string();
    let token = generate_token();
    let created = TimeString::now().to_string();

    sqlx::query!(
        r#"INSERT INTO "link_tokens" ("token", "user_id", "label", "created", "expires")
        VALUES (?, ?, ?, ?, ?)"#,
        token,
        user_id_str,
        add.label,
        created,
        add.expires,
    )
    .execute(db())
    .await?;

    Ok(LinkToken {
        token,
        label: add.label,
        created,
        expires: add.expires,
    })
}

pub async fn delete(req: &Request) -> Result<(), Error> {
    let delete: LinkTokenDelete = decode_structured_message(req.message.clone())?;
    let user_id_str = delete.user_id.to_string();

    sqlx::query!(
        r#"DELETE FROM "link_tokens" WHERE "token" = ? AND "user_id" = ?"#,
        delete.token,
        user_id_str,
    )
    .execute(db())
    .await?;

    Ok(())
}

pub(crate) fn generate_token() -> String {
    use base64::Engine;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.r#gen()).collect();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes)
}
