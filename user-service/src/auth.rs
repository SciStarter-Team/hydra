use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{Session, Signup},
    service::log_activity,
    shared::{db, hash_identifier, hash_identifier_capfirst, hash_identifier_lower},
};
use ulid::Ulid;

use crate::sessions::read_session;

/// Sign the current session up for a new account
pub async fn signup(req: &Request) -> Result<Option<Ulid>, Error> {
    let form: Signup = flexbuffers::from_slice(&req.message.payload)?;
    let mut session = read_session(&form.session_id).await?;

    let user_id = Ulid::new();
    let user_id_str = user_id.to_string();
    let handle = form.handle;
    let email = form.email;
    let password = djangohashers::make_password(&form.password);
    let newsletter = form.newsletter;
    let zipcode = form.zipcode;
    let protected = form.protected;
    let point = form.point;

    let mut tx = db().begin().await?;

    sqlx::query!(
        r#"insert into "users" (
          "user_id",
          "username",
          "handle",
          "primary_email",
          "verified",
          "password",
          "newsletter",
          "zipcode",
          "protected",
          "point"
        )
        values (
          ?1,
          ?2,
          ?3,
          ?2,
          false,
          ?4,
          ?5,
          ?6,
          ?7,
          GeomFromGeoJSON(?8)
        )"#,
        user_id_str,
        email,
        handle,
        password,
        newsletter,
        zipcode,
        protected,
        point,
    )
    .execute(&mut *tx)
    .await?;

    let hashed = hash_identifier(&email);
    let hashed_lower = hash_identifier_lower(&email);
    let hashed_capfirst = hash_identifier_capfirst(&email);

    sqlx::query!(
        r#"insert into "emails" (
          "user_id",
          "email",
          "verified",
          "hash",
          "hash_lower",
          "hash_capfirst"
        )
        values (
          ?,
          ?,
          false,
          ?,
          ?,
          ?
        )"#,
        user_id_str,
        email,
        hashed,
        hashed_lower,
        hashed_capfirst,
    )
    .execute(&mut *tx)
    .await?;

    for tag in form.tags.iter() {
        let tag = tag.to_lowercase();

        sqlx::query!(
            r#"insert into "user_tags" (
              "user_id",
              "tag"
            )
            values (
              ?,
              ?
            )
            on conflict ("user_id", "tag") do nothing"#,
            user_id_str,
            tag,
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    log_activity(None, "signup", user_id, None, None).await.ok();

    Ok(Some(user_id))
}

pub async fn login(req: &Request) -> Result<Option<Ulid>, Error> {
    todo!()
}

pub async fn logout(req: &Request) -> Result<Option<Ulid>, Error> {
    todo!()
}
