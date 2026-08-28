//! Applications to host an Acts of Science Connected event at a library.
//!
//! Ported from orrery's `LibraryActsOfScienceConnectedSignup`. The form is
//! public and lives on a curated page; staff read the applications back as a
//! CSV. Rows are written once and never edited, so there are only two
//! operations.

use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::dashboard::ActsConnectedSignup, service::decode_structured_message, shared::db,
    timestring::TimeString,
};
use ulid::Ulid;

/// Record one application (`user.acts_connected.record`).
///
/// The id and timestamp are stamped here rather than accepted from the caller,
/// so a submission cannot claim to have arrived at some other time or overwrite
/// an existing row.
pub async fn record(req: &Request) -> Result<(), Error> {
    let signup: ActsConnectedSignup =
        decode_structured_message(req.message.clone()).unwrap_or_default();

    let signup_id = Ulid::new().to_string();
    let recorded = TimeString::now();

    sqlx::query!(
        r#"INSERT INTO "acts_of_science_connected_signups" (
            "signup_id", "recorded",
            "library_name", "address1", "address2", "city", "state", "zip", "country",
            "contact_first", "contact_last", "contact_email", "contact_phone",
            "service_area", "seating_capacity", "tech_support",
            "event1aft", "event1eve", "event2aft", "event2eve",
            "event3aft", "event3mid", "event3eve", "event4aft", "event4eve",
            "event5aft", "event5eve", "eventpromote",
            "additional_programming", "why_interested", "requirements",
            "wired_ethernet", "available_devices", "comments", "request_grant"
        ) VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
        )"#,
        signup_id,
        recorded,
        signup.library_name,
        signup.address1,
        signup.address2,
        signup.city,
        signup.state,
        signup.zip,
        signup.country,
        signup.contact_first,
        signup.contact_last,
        signup.contact_email,
        signup.contact_phone,
        signup.service_area,
        signup.seating_capacity,
        signup.tech_support,
        signup.event1aft,
        signup.event1eve,
        signup.event2aft,
        signup.event2eve,
        signup.event3aft,
        signup.event3mid,
        signup.event3eve,
        signup.event4aft,
        signup.event4eve,
        signup.event5aft,
        signup.event5eve,
        signup.eventpromote,
        signup.additional_programming,
        signup.why_interested,
        signup.requirements,
        signup.wired_ethernet,
        signup.available_devices,
        signup.comments,
        signup.request_grant,
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Every application, oldest first (`user.acts_connected.list`).
///
/// Unpaginated: this is a hand-filled form for one programme, so the table
/// holds hundreds of rows at most, and the staff export wants all of them.
pub async fn list(_req: &Request) -> Result<Vec<ActsConnectedSignup>, Error> {
    let rows = sqlx::query_as!(
        ActsConnectedSignup,
        r#"SELECT
            "signup_id", "recorded",
            "library_name", "address1", "address2", "city", "state", "zip", "country",
            "contact_first", "contact_last", "contact_email", "contact_phone",
            "service_area", "seating_capacity", "tech_support",
            "event1aft", "event1eve", "event2aft", "event2eve",
            "event3aft", "event3mid", "event3eve", "event4aft", "event4eve",
            "event5aft", "event5eve", "eventpromote",
            "additional_programming", "why_interested", "requirements",
            "wired_ethernet", "available_devices", "comments", "request_grant"
           FROM "acts_of_science_connected_signups"
           ORDER BY "recorded""#
    )
    .fetch_all(db())
    .await?;

    Ok(rows)
}
