use std::{collections::BTreeMap, time::Duration};

use common::{message::user::Session, service::request_typed, shared::nats};
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

    /// Waits for user-service to confirm the delete rather than publishing it
    /// and moving on. The response to a logout is followed at once by the
    /// browser's next request, and were the delete still in flight, that
    /// request could load the session, find it logged in, and store it back.
    #[tracing::instrument]
    async fn remove_session<'a>(&'a self, session_id: &'a str) -> Result<()> {
        request_typed::<()>("user.session.delete", session_id.to_owned())
            .await
            .map_err(InternalServerError)
    }
}

/// Training steps a visitor earned while signed out, held in the session for
/// the account they will be recorded on.
///
/// A tracked go link drives a signed-in visitor's training on the spot
/// ([`crate::gourl`]); a visitor with no account has it held here instead,
/// and whichever account this session then signs up for or signs in to is
/// driven through it. The training page reads what is held to acknowledge a
/// completion the site has no row for yet. Orrery keeps the same thing under
/// its session's `pending_transitions` key.
pub mod pending_transitions {
    use common::message::user::{PendingTransition, WorkflowType};
    use poem::session::Session;

    const KEY: &str = "pending_transitions";

    /// Hold a step for the account to come. The step is stored as the user
    /// service names it, so that what the page reads back and what the
    /// service records agree whichever spelling the link used. Holding the
    /// same step twice holds it once.
    pub fn stash(session: &Session, workflow: WorkflowType, step: &str) {
        let held = PendingTransition {
            workflow,
            step: workflow.canonical_step(step),
        };

        let mut all = list(session);

        if all.contains(&held) {
            return;
        }

        all.push(held);
        session.set(KEY, all);
    }

    /// Everything held, in the order it was held, without releasing it. The
    /// signup lanes pass this to `create_account`, which records it from the
    /// row that creates the user, and clear it once that has succeeded.
    pub fn list(session: &Session) -> Vec<PendingTransition> {
        session
            .get::<Vec<PendingTransition>>(KEY)
            .unwrap_or_default()
    }

    /// The steps held for one training, as the user service names them.
    pub fn steps_for(session: &Session, workflow: WorkflowType) -> Vec<String> {
        list(session)
            .into_iter()
            .filter(|held| held.workflow == workflow)
            .map(|held| held.step)
            .collect()
    }

    /// Release everything held and return it, for a lane that has an
    /// account in hand to record it on.
    pub fn clear(session: &Session) -> Vec<PendingTransition> {
        let all = list(session);
        session.remove(KEY);
        all
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The Foundations tutorial's own link says `hour1`, orrery's name
        /// for the step; it is held as `tutorial`, this side's, which is what
        /// the page asks for and what the user service records.
        #[test]
        fn a_held_step_is_spelled_the_way_the_user_service_spells_it() {
            let session = Session::default();

            stash(&session, WorkflowType::Foundations, "hour1");

            assert_eq!(
                steps_for(&session, WorkflowType::Foundations),
                vec!["tutorial"]
            );
        }

        #[test]
        fn following_the_same_link_twice_holds_the_step_once() {
            let session = Session::default();

            stash(&session, WorkflowType::Foundations, "hour1");
            stash(&session, WorkflowType::Foundations, "tutorial");

            assert_eq!(list(&session).len(), 1);
        }

        #[test]
        fn steps_are_held_per_training_in_the_order_they_arrived() {
            let session = Session::default();

            stash(&session, WorkflowType::K12, "tutorial");
            stash(&session, WorkflowType::Foundations, "hour1");
            stash(&session, WorkflowType::Foundations, "squirrel");

            assert_eq!(
                steps_for(&session, WorkflowType::Foundations),
                vec!["tutorial", "squirrel"]
            );
            assert_eq!(steps_for(&session, WorkflowType::K12), vec!["tutorial"]);
            assert_eq!(list(&session).len(), 3);
        }

        #[test]
        fn clearing_releases_everything_once() {
            let session = Session::default();
            stash(&session, WorkflowType::Foundations, "hour1");

            let released = clear(&session);

            assert_eq!(released.len(), 1);
            assert!(list(&session).is_empty());
            assert!(clear(&session).is_empty());
        }

        #[test]
        fn a_session_holding_nothing_reads_as_empty() {
            let session = Session::default();

            assert!(list(&session).is_empty());
            assert!(steps_for(&session, WorkflowType::Foundations).is_empty());
        }
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
