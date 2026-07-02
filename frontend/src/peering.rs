use std::sync::LazyLock;

use anyhow::Error;
use async_nats::Message;
use common::{nats_headers, service::send_with_headers, shared::nats};
use futures::StreamExt;
use ulid::Ulid;

pub static SERVER_ID: LazyLock<Ulid> = LazyLock::new(Ulid::new);
pub static SERVER_ID_STR: LazyLock<String> = LazyLock::new(|| SERVER_ID.to_string());

/// Implementors of this trait support peer-to-peer synchronization of
/// stored instances.
#[allow(async_fn_in_trait)]
pub trait LoadStorePeered:
    std::fmt::Debug + serde::Serialize + for<'d> serde::Deserialize<'d>
{
    //type DatabaseKey<'k>;
    type DatabaseKey: Clone + serde::Serialize + serde::de::DeserializeOwned; // Should be DatabaseKey<'k> but as of Rust 1.87 that triggers https://github.com/rust-lang/rust/issues/100013

    const GROUP: &str;
    const SPECIFIC: &str = "";

    /// Loads the Self with the given id
    ///
    /// This is the main entry point for loading data.
    ///
    /// Overriding the default implementation is not recommended.
    #[tracing::instrument(skip(key))]
    //async fn load<'k>(key: Self::DatabaseKey<'k>) -> Result<Self, Error> {
    async fn load(key: Self::DatabaseKey) -> Result<Self, Error> {
        Self::read_from_database(key).await
    }

    /// Stores self, updating peers with the change as well.
    ///
    /// This is the main entry point for storing data.
    ///
    /// Overriding the default implementation is not recommended.
    #[tracing::instrument]
    async fn store(&self) -> Result<(), Error> {
        self.save_to_database().await?;
        send_with_headers(
            peering_subject_store::<Self>(),
            nats_headers! {
                "Source": SERVER_ID_STR.as_str(),
            },
            self,
        )
        .await?;
        Ok(())
    }

    /// Override the default implementation if you want to handle
    /// custom signals for this type.
    ///
    /// Signals are a wrapper around standard NATS messaging, with
    /// automatic message scoping, and the the sender of a message
    /// exempted from receiving it.
    #[tracing::instrument]
    async fn receive(
        _signal: String,
        _specific: Option<String>,
        _msg: Message,
    ) -> Result<(), Error> {
        Ok(())
    }

    /// Send a custom signal for this type.
    ///
    /// Signal names should consist of alphanumeric characters,
    /// dashes, and underscores only. Particularly, do not use period
    /// (.) characters.
    #[tracing::instrument(skip(signal, msg))]
    async fn transmit(signal: impl AsRef<str>, msg: impl serde::Serialize) -> Result<(), Error> {
        send_with_headers(
            peering_subject_signal::<Self>(signal),
            nats_headers! {
                "Source": SERVER_ID_STR.as_str(),
            },
            msg,
        )
        .await?;

        Ok(())
    }

    /// Spawn the returned Future as a background task to enable
    /// peering for this type.
    ///
    /// Overriding the default implementation is not recommended.
    #[tracing::instrument]
    async fn peering_task() -> Result<(), Error> {
        let client = nats();

        let store_subject = peering_subject_store::<Self>();
        let load_subject = peering_subject_load::<Self>();
        let delete_subject = peering_subject_delete::<Self>();

        let mut updates = client.subscribe(peering_subject_listen::<Self>()).await?;

        while let Some(msg) = updates.next().await {
            if let Some(headers) = &msg.headers {
                if headers
                    .get("Source")
                    .is_some_and(|v| v.as_str() != SERVER_ID_STR.as_str())
                {
                    let msg_subject = msg.subject.to_string();
                    if msg_subject == store_subject {
                        let value: Self = match flexbuffers::from_slice(&msg.payload) {
                            Ok(val) => val,
                            Err(err) => {
                                eprintln!(
                                    "({}:{}) Unable to decode message payload to record in peering_task: {}",
                                    Self::GROUP,
                                    Self::SPECIFIC,
                                    err
                                );
                                return Ok(());
                            }
                        };

                        if let Err(err) = value.save_to_database().await {
                            eprintln!(
                                "({}:{}) Error in save_to_database(): {}",
                                Self::GROUP,
                                Self::SPECIFIC,
                                err
                            );
                        }
                    } else if msg_subject == delete_subject {
                        let key: Self::DatabaseKey = match flexbuffers::from_slice(&msg.payload) {
                            Ok(key) => key,
                            Err(err) => {
                                eprintln!(
                                    "({}:{}) Unable to decode delete key in peering_task: {}",
                                    Self::GROUP,
                                    Self::SPECIFIC,
                                    err
                                );
                                continue;
                            }
                        };

                        if let Err(err) = Self::delete_from_database(key).await {
                            eprintln!(
                                "({}:{}) Error in delete_from_database(): {}",
                                Self::GROUP,
                                Self::SPECIFIC,
                                err
                            );
                        }
                    } else if msg_subject == load_subject {
                        unimplemented!("Peer loading")
                    } else {
                        if let Some(signal) = msg.subject.split_once(".signal.").map(|(_, s)| s) {
                            let (signal, specific) =
                                if let Some((sig, spec)) = signal.split_once('.') {
                                    (sig.to_string(), Some(spec.to_string()))
                                } else {
                                    (signal.to_string(), None)
                                };

                            if let Err(err) = Self::receive(signal, specific, msg).await {
                                eprintln!(
                                    "({}:{}) Error during message processing: {}",
                                    Self::GROUP,
                                    Self::SPECIFIC,
                                    err
                                );
                            }
                        }
                        // else, silently ignore the message, it's not a custom signal
                    }
                }
                // else, silently discard the message since it was sent from the same server
            } else {
                eprintln!(
                    "({}:{}) Message has no headers: {}",
                    Self::GROUP,
                    Self::SPECIFIC,
                    msg.subject.as_str()
                );
            }
        }

        Ok(())
    }

    /// Saves self to the local database, without any peer involvement.
    ///
    /// Rather than calling this method directly, it should be invoked
    /// via the store() method for proper peer-to-peer synchronization.
    async fn save_to_database(&self) -> Result<(), Error>;

    /// Loads the Self with the given id from the local database, without any peer involvement.
    ///
    /// Rather than calling this method directly, it should be invoked
    /// via the load() method for proper peer-to-peer synchronization.
    //async fn read_from_database<'k>(key: Self::DatabaseKey<'k>) -> Result<Self, Error>;
    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, Error>;

    /// Deletes the record with the given key from the local database, without any peer involvement.
    ///
    /// Rather than calling this method directly, it should be invoked
    /// via the delete() method for proper peer-to-peer synchronization.
    async fn delete_from_database(_key: Self::DatabaseKey) -> Result<(), Error> {
        unimplemented!("delete_from_database not implemented for this type")
    }

    /// Deletes the record with the matching key, updating peers with the change as well.
    ///
    /// This is the main entry point for deleting data.
    ///
    /// Overriding the default implementation is not recommended.
    #[tracing::instrument(skip(key))]
    async fn delete(key: Self::DatabaseKey) -> Result<(), Error> {
        Self::delete_from_database(key.clone()).await?;
        send_with_headers(
            peering_subject_delete::<Self>(),
            nats_headers! {
                "Source": SERVER_ID_STR.as_str(),
            },
            &key,
        )
        .await?;
        Ok(())
    }
}

// Seems like the next two functions *should* be able to be const fn,
// but the compiler complains. There's probably a way.

/// Returns the NATS subject used for peering stores on Y.
#[tracing::instrument]
fn peering_subject_store<T: LoadStorePeered>() -> String {
    if T::SPECIFIC.is_empty() {
        format!("frontend.{}.store", T::GROUP)
    } else {
        format!("frontend.{}.store.{}", T::GROUP, T::SPECIFIC)
    }
}

/// Returns the NATS subject used for peering loads on T.
#[tracing::instrument]
fn peering_subject_load<T: LoadStorePeered>() -> String {
    if T::SPECIFIC.is_empty() {
        format!("frontend.{}.load", T::GROUP)
    } else {
        format!("frontend.{}.load.{}", T::GROUP, T::SPECIFIC)
    }
}

/// Returns the NATS subject used for peering deletes on T.
#[tracing::instrument]
fn peering_subject_delete<T: LoadStorePeered>() -> String {
    if T::SPECIFIC.is_empty() {
        format!("frontend.{}.delete", T::GROUP)
    } else {
        format!("frontend.{}.delete.{}", T::GROUP, T::SPECIFIC)
    }
}

/// Returns the NATS subject used for listening for peering messages
#[tracing::instrument]
fn peering_subject_listen<T: LoadStorePeered>() -> String {
    format!("frontend.{}.>", T::GROUP)
}

/// Returns the NATS subject used for peering the custom signal on T.
#[tracing::instrument(skip(signal))]
fn peering_subject_signal<T: LoadStorePeered>(signal: impl AsRef<str>) -> String {
    if T::SPECIFIC.is_empty() {
        format!("frontend.{}.signal.{}", T::GROUP, signal.as_ref())
    } else {
        format!(
            "frontend.{}.signal.{}.{}",
            T::GROUP,
            signal.as_ref(),
            T::SPECIFIC
        )
    }
}
