//! GET `/staff/acts-of-science-connected-signups` — the Acts of Science
//! Connected hosting applications as a CSV. Ported from orrery's
//! `acts_of_science_connected_signups`.
//!
//! orrery served this from `/l/acts-of-science-connected-signups`, next to the
//! public form. hydra puts the staff half under `/staff/`, where one gate
//! covers every staff report, and leaves the public form on its own path —
//! matching what the Library Network sign-up already does.

use common::{message::dashboard::ActsConnectedSignup, service::request};
use jiff::{Timestamp, tz::TimeZone};
use poem::{Response, handler};

use super::csv_field;

/// The columns, in the order orrery's `DictWriter` emitted them (its field
/// list came from the model, which is this order).
const COLUMNS: &[&str] = &[
    "recorded",
    "library_name",
    "address1",
    "address2",
    "city",
    "state",
    "zip",
    "country",
    "contact_first",
    "contact_last",
    "contact_email",
    "contact_phone",
    "service_area",
    "seating_capacity",
    "tech_support",
    "event1aft",
    "event1eve",
    "event2aft",
    "event2eve",
    "event3aft",
    "event3mid",
    "event3eve",
    "event4aft",
    "event4eve",
    "event5aft",
    "event5eve",
    "eventpromote",
    "additional_programming",
    "why_interested",
    "requirements",
    "wired_ethernet",
    "available_devices",
    "comments",
    "request_grant",
];

/// The value of each column, in the same order.
fn fields(signup: &ActsConnectedSignup) -> Vec<&str> {
    vec![
        signup.recorded.as_str(),
        signup.library_name.as_str(),
        signup.address1.as_str(),
        signup.address2.as_str(),
        signup.city.as_str(),
        signup.state.as_str(),
        signup.zip.as_str(),
        signup.country.as_str(),
        signup.contact_first.as_str(),
        signup.contact_last.as_str(),
        signup.contact_email.as_str(),
        signup.contact_phone.as_str(),
        signup.service_area.as_str(),
        signup.seating_capacity.as_str(),
        signup.tech_support.as_str(),
        signup.event1aft.as_str(),
        signup.event1eve.as_str(),
        signup.event2aft.as_str(),
        signup.event2eve.as_str(),
        signup.event3aft.as_str(),
        signup.event3mid.as_str(),
        signup.event3eve.as_str(),
        signup.event4aft.as_str(),
        signup.event4eve.as_str(),
        signup.event5aft.as_str(),
        signup.event5eve.as_str(),
        signup.eventpromote.as_str(),
        signup.additional_programming.as_str(),
        signup.why_interested.as_str(),
        signup.requirements.as_str(),
        signup.wired_ethernet.as_str(),
        signup.available_devices.as_str(),
        signup.comments.as_str(),
        signup.request_grant.as_str(),
    ]
}

/// GET `/staff/acts-of-science-connected-signups` — the CSV.
#[handler]
#[tracing::instrument]
pub async fn signups() -> Result<Response, anyhow::Error> {
    let signups: Vec<ActsConnectedSignup> = request("user.acts_connected.list", ()).await?;

    let mut csv = COLUMNS.join(",");
    csv.push('\n');

    for signup in &signups {
        let line: Vec<String> = fields(signup).iter().map(|f| csv_field(f)).collect();
        csv.push_str(&line.join(","));
        csv.push('\n');
    }

    let today = Timestamp::now().to_zoned(TimeZone::UTC).date();

    Ok(Response::builder()
        .content_type("text/csv")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"connected_signups_{today}.csv\""),
        )
        .body(csv))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_column_has_a_field() {
        assert_eq!(COLUMNS.len(), fields(&ActsConnectedSignup::default()).len());
    }
}
