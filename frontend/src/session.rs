use std::{collections::BTreeMap, time::Duration};

use common::{message::user::Session, shared::nats};
use jiff::Timestamp;
use poem::{
    Result,
    error::{BadRequest, InternalServerError},
    session::SessionStorage,
};
use sqlx::types::JsonValue;

pub struct UserServiceSessionStorage;

impl SessionStorage for UserServiceSessionStorage {
    async fn load_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> Result<Option<BTreeMap<String, JsonValue>>> {
        let session_id = flexbuffers::to_vec(session_id.to_owned()).map_err(BadRequest)?;
        let nats = nats();

        if let Some(msg) = nats
            .request("user.session.retrieve", session_id.into())
            .await
            .ok()
        {
            let session: Session =
                flexbuffers::from_slice(&msg.payload).map_err(InternalServerError)?;

            if let Some(expires) = &session.expires {
                if expires.parse::<Timestamp>().map_err(InternalServerError)? < Timestamp::now() {
                    return Ok(None);
                }
            }

            Ok(flexbuffers::from_slice(&session.data).map_err(InternalServerError)?)
        } else {
            Ok(None)
        }
    }

    async fn update_session<'a>(
        &'a self,
        session_id: &'a str,
        entries: &'a BTreeMap<String, JsonValue>,
        expires: Option<Duration>,
    ) -> Result<()> {
        let session_id = session_id.to_owned();
        let expires = expires.map(|d| (Timestamp::now() + d).to_string());
        let data = flexbuffers::to_vec(entries).map_err(BadRequest)?.into();

        let session = Session {
            session_id,
            expires,
            data,
        };

        let payload = flexbuffers::to_vec(session)
            .map_err(InternalServerError)?
            .into();

        let nats = nats();

        nats.publish("user.session.store", payload)
            .await
            .map_err(InternalServerError)?;

        Ok(())
    }

    async fn remove_session<'a>(&'a self, session_id: &'a str) -> Result<()> {
        let session_id = flexbuffers::to_vec(session_id.to_owned()).map_err(BadRequest)?;
        let nats = nats();

        nats.publish("user.session.delete", session_id.into())
            .await
            .map_err(InternalServerError)?;

        Ok(())
    }
}
