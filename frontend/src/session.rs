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
            if let Some(session) = flexbuffers::from_slice::<Option<Session>>(&msg.payload)
                .map_err(InternalServerError)?
            {
                if let Some(expires) = &session.expires {
                    if expires.parse::<Timestamp>().map_err(InternalServerError)? < Timestamp::now()
                    {
                        return Ok(None);
                    }
                }

                Ok(Some(session.decode()?))
            } else {
                Ok(None)
            }
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
        let expires = expires.map(|d| (Timestamp::now() + d).to_string());

        let session = Session::new(session_id, expires, entries)?;

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

/// Session tag management helper functions
pub mod session_tags {
    use poem::session::Session;
    use std::collections::BTreeSet;

    /// Add a tag to the session
    pub fn add(session: &Session, tag: &str) {
        let mut tags = get_set(session);
        let tag_lower = tag.to_lowercase();

        if tags.insert(tag_lower) {
            // Only update session if tag was actually added
            session.set("session_tags", tags);
        }
    }

    /// Get all tags from the session
    pub fn list(session: &Session) -> Vec<String> {
        get_set(session).into_iter().collect()
    }

    /// Clear all tags from the session and return them
    pub fn clear(session: &Session) -> Vec<String> {
        let tags = list(session);
        session.remove("session_tags");
        tags
    }

    /// Remove a specific tag from the session
    pub fn remove(session: &Session, tag: &str) {
        let mut tags = get_set(session);
        let tag_lower = tag.to_lowercase();

        if tags.remove(&tag_lower) {
            // Only update session if tag was actually removed
            session.set("session_tags", tags);
        }
    }

    /// Helper function to get tags as BTreeSet
    fn get_set(session: &Session) -> BTreeSet<String> {
        session
            .get::<BTreeSet<String>>("session_tags")
            .unwrap_or_default()
    }
}
