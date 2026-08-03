use std::{collections::BTreeMap, time::Duration};

use common::{message::user::Session, shared::nats};
use jiff::Timestamp;
use poem::{
    Result,
    error::{BadRequest, InternalServerError},
    session::SessionStorage,
};
use sqlx::types::JsonValue;

#[derive(Debug)]
pub struct UserServiceSessionStorage;

impl SessionStorage for UserServiceSessionStorage {
    #[tracing::instrument]
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

    #[tracing::instrument]
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

    #[tracing::instrument]
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
    use common::message::user::normalize_tag;
    use poem::session::Session;
    use std::collections::BTreeSet;

    /// Add a tag to the session
    ///
    /// Normalized on the way in, at the door these tags arrive through, so
    /// that a `?tag=` a partner typed as `#GirlScout` is the same tag from
    /// here on -- to `remove` below, to the protection rule, and to both
    /// columns it eventually lands in.
    pub fn add(session: &Session, tag: &str) {
        let mut tags = get_set(session);
        let tag_lower = normalize_tag(tag);

        if tag_lower.is_empty() {
            return;
        }

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
        let tag_lower = normalize_tag(tag);

        if tags.remove(&tag_lower) {
            // Only update session if tag was actually removed
            session.set("session_tags", tags);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A go link stamps orrery's comma-separated `signup_tags`; it has to
        /// reach signup the same way a `?tag=` would.
        #[test]
        fn go_link_signup_tags_are_folded_in_and_normalized() {
            let session = Session::default();
            session.set("signup_tags", "girlscout, #Library , ,nasa");

            let mut tags = list(&session);
            tags.sort();

            assert_eq!(tags, vec!["girlscout", "library", "nasa"]);
        }

        #[test]
        fn folded_tags_merge_with_tags_added_the_ordinary_way() {
            let session = Session::default();
            add(&session, "girlscout");
            session.set("signup_tags", "nasa,girlscout");

            let mut tags = clear(&session);
            tags.sort();

            assert_eq!(tags, vec!["girlscout", "nasa"]);
            assert!(list(&session).is_empty(), "clear must empty both keys");
        }

        /// `/go/girlscout-signup` carries `signup_tags=girlscout`, and
        /// `girlscout` is a PROTECTED_SIGNUP_TAG. The tag has to survive the
        /// fold, because what rides on it is whether a minor's account is
        /// created protected.
        #[test]
        fn a_go_link_protection_tag_reaches_the_protection_decision() {
            use common::message::user::signup_needs_protection;

            let session = Session::default();
            session.set("signup_tags", "girlscout");

            assert!(signup_needs_protection(&list(&session)));
        }

        /// The drain is what makes this true: were `signup_tags` merged on
        /// every read instead, a removed tag would come back.
        #[test]
        fn a_removed_tag_stays_removed() {
            let session = Session::default();
            session.set("signup_tags", "girlscout,nasa");

            remove(&session, "girlscout");

            assert_eq!(list(&session), vec!["nasa"]);
        }
    }

    /// Helper function to get tags as BTreeSet
    ///
    /// Also drains `signup_tags`, the comma-separated key a tracked go link
    /// stamps into the session (orrery's `CountedRedirect.session_set_value`;
    /// see [`crate::gourl`]). Draining rather than merging on every read keeps
    /// `remove` and `clear` meaningful — a tag folded in once behaves from
    /// then on exactly like one added through `add`.
    fn get_set(session: &Session) -> BTreeSet<String> {
        let mut tags = session
            .get::<BTreeSet<String>>("session_tags")
            .unwrap_or_default();

        if let Some(raw) = session.get::<String>("signup_tags") {
            session.remove("signup_tags");

            let folded = raw
                .split(',')
                .map(normalize_tag)
                .filter(|tag| !tag.is_empty())
                .fold(false, |changed, tag| tags.insert(tag) || changed);

            if folded {
                session.set("session_tags", tags.clone());
            }
        }

        tags
    }
}
