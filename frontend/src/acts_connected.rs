//! The public Acts of Science Connected hosting application.
//!
//! Ported from orrery's `acts_of_science_connected_form`, at the same path so
//! the curated page that carries the form keeps working after the cutover.
//! Like orrery's, this is POST-only: the form itself is authored on
//! `/library-acts-of-science-connected-apply`, and this endpoint only receives
//! it.
//!
//! The application is stored by the user service, and the applicant gets a
//! confirmation. The staff export lives at
//! `/staff/acts-of-science-connected-signups`.

use common::{
    message::{dashboard::ActsConnectedSignup, email::EmailRecipient},
    service::{send, send_email},
};
use poem::{IntoResponse, Response, handler, web::Form, web::Redirect};
use serde::Deserialize;

/// Where the applicant lands afterwards. The anchor is what orrery used, and
/// the page shows its confirmation on it.
const SUCCESS_URL: &str = "/library-acts-of-science-connected-apply#success";

/// The submitted form.
///
/// Every field is optional as far as parsing goes — orrery read them all with
/// a default of `""` and only `library_name` was required — so a form that
/// gains or loses a field on the curated page does not start rejecting
/// submissions.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ApplicationForm {
    library_name: String,
    address1: String,
    address2: String,
    city: String,
    state: String,
    zip: String,
    country: String,

    contact_first: String,
    contact_last: String,
    contact_email: String,
    contact_phone: String,

    service_area: String,
    seating_capacity: String,
    tech_support: String,

    event1aft: String,
    event1eve: String,
    event2aft: String,
    event2eve: String,
    event3aft: String,
    event3mid: String,
    event3eve: String,
    event4aft: String,
    event4eve: String,
    event5aft: String,
    event5eve: String,
    eventpromote: String,

    additional_programming: String,
    why_interested: String,
    requirements: String,
    wired_ethernet: String,
    available_devices: String,
    comments: String,
    request_grant: String,
}

/// POST `/l/acts-of-science-connected-apply` — record an application.
#[handler]
#[tracing::instrument]
pub async fn submit(Form(form): Form<ApplicationForm>) -> Result<Response, anyhow::Error> {
    let contact_email = form.contact_email.trim().to_string();

    let signup = ActsConnectedSignup {
        // Stamped by the user service, not claimed here.
        signup_id: String::new(),
        recorded: String::new(),

        library_name: form.library_name,
        address1: form.address1,
        address2: form.address2,
        city: form.city,
        state: form.state,
        zip: form.zip,
        country: form.country,

        contact_first: form.contact_first,
        contact_last: form.contact_last,
        contact_email: contact_email.clone(),
        contact_phone: form.contact_phone,

        service_area: form.service_area,
        seating_capacity: form.seating_capacity,
        tech_support: form.tech_support,

        event1aft: form.event1aft,
        event1eve: form.event1eve,
        event2aft: form.event2aft,
        event2eve: form.event2eve,
        event3aft: form.event3aft,
        event3mid: form.event3mid,
        event3eve: form.event3eve,
        event4aft: form.event4aft,
        event4eve: form.event4eve,
        event5aft: form.event5aft,
        event5eve: form.event5eve,
        eventpromote: form.eventpromote,

        additional_programming: form.additional_programming,
        why_interested: form.why_interested,
        requirements: form.requirements,
        wired_ethernet: form.wired_ethernet,
        available_devices: form.available_devices,
        comments: form.comments,
        // orrery's default, for a form that omits the question entirely.
        request_grant: if form.request_grant.is_empty() {
            String::from("No")
        } else {
            form.request_grant
        },
    };

    send("user.acts_connected.record", signup).await?;

    // A confirmation, when there is somewhere to send it. The application is
    // already stored by this point, so a mail failure does not lose it.
    if !contact_email.is_empty() {
        if let Err(error) = send_email(
            vec![EmailRecipient::Email(contact_email.clone())],
            "Your Acts of Science Connected application",
            "<p>Thank you for applying to host an Acts of Science Connected event \
             at your library.</p>\
             <p>We have your application and will be in touch about which dates \
             we can support. If you need to change anything in the meantime, \
             reply to this message.</p>\
             <p>&mdash; The SciStarter team</p>",
            Vec::<String>::new(),
            None,
            None,
        )
        .await
        {
            tracing::error!(%error, "Acts of Science Connected confirmation failed to send");
        }
    }

    Ok(Redirect::see_other(SUCCESS_URL).into_response())
}
