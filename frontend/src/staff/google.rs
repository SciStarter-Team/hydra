//! GET/POST `/staff/google-authorize` and GET `/staff/google-authorized` —
//! the Google authorization handshake and the imports it exists to run. Ported
//! from orrery's `google_authorize` / `google_authorized`.
//!
//! Staff maintain the library directory in a Google Sheet. Google will not hand
//! out a sheet without a user's consent, so a staff member grants it here and
//! the resulting token is used immediately to run the import — the token is
//! never stored, which is why the authorization is redone each time rather than
//! refreshed in the background.
//!
//! The import streams its progress as it goes. It is a request per row against
//! a geocoder, so a sheet with a few hundred new libraries in it takes minutes,
//! and a response that arrives only at the end would look like a hung page.

use common::{
    message::dashboard::{LibraryDirectoryEntry, LibraryDirectoryRecordResponse},
    service::request,
};
use poem::{
    IntoResponse, Response, handler,
    session::Session,
    web::{Query, Redirect},
};
use serde::Deserialize;

use crate::component::html_escape;
use crate::geocode::geocode_parts;

/// The OAuth client staff authorize. Shared with orrery, whose redirect URI is
/// already registered against it, so the handshake keeps working through the
/// cutover without a change at Google's end.
const CLIENT_ID: &str = "7554170380-smkdjcvbeo2c3f65sqth7l47cuunchiq.apps.googleusercontent.com";

/// The client secret paired with [`CLIENT_ID`].
const GOOGLE_AUTH_SECRET: &str = env!("GOOGLE_AUTH_SECRET");

/// Must match a redirect URI registered against [`CLIENT_ID`].
const REDIRECT_URI: &str = "https://scistarter.org/staff/google-authorized";

/// Read and write, because the same authorization is used for imports today
/// and is what orrery asked for.
const SCOPES: &str =
    "https://www.googleapis.com/auth/spreadsheets https://www.googleapis.com/auth/drive";

/// The sheet the library directory is maintained in.
const LIBRARY_SHEET: &str = "1Wn7t23TCHtuAoHIWiW6nAw2iyhAv7Q6pKqixUoIVN9Q";

/// The columns of that sheet: name, address, city, state, zip, org type, url.
const LIBRARY_RANGE: &str = "A:G";

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct AuthorizedQuery {
    code: String,
    error: String,
    state: String,
}

/// Session key holding the `state` value we sent Google, so the code that
/// comes back can be tied to the request that asked for it.
const STATE_KEY: &str = "google_authorize_state";

/// GET `/staff/google-authorize` — the button that starts the handshake.
#[handler]
pub async fn authorize_form() -> Response {
    let html = "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>Authorize Google Integrations</title>\
         <style>body{font-family:sans-serif;margin:2rem;max-width:560px;}</style>\
         </head><body><h1>Authorize Google integrations</h1>\
         <p>Grants SciStarter access to the Google Sheets that staff maintain, \
         then imports any libraries the directory does not have yet. \
         The authorization is used once and not kept.</p>\
         <form method=\"post\"><button type=\"submit\">Authorize Google Integrations</button></form>\
         </body></html>";

    Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(html)
}

/// POST `/staff/google-authorize` — hand off to Google's consent screen.
#[handler]
pub async fn authorize(session: &Session) -> Response {
    // A one-off value Google echoes back, remembered in this browser's session
    // so that a `code` arriving from anywhere else is refused.
    let state = ulid::Ulid::new().to_string();
    session.set(STATE_KEY, &state);

    let url = format!(
        "https://accounts.google.com/o/oauth2/auth\
         ?client_id={client_id}\
         &redirect_uri={redirect}\
         &response_type=code\
         &scope={scope}\
         &access_type=offline\
         &state={state}",
        client_id = urlencoding::encode(CLIENT_ID),
        redirect = urlencoding::encode(REDIRECT_URI),
        scope = urlencoding::encode(SCOPES),
    );

    Redirect::see_other(url).into_response()
}

/// What Google returns from the token endpoint.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}

/// One row of the library sheet.
#[derive(Debug, Deserialize)]
struct SheetValues {
    #[serde(default)]
    values: Vec<Vec<String>>,
}

/// GET `/staff/google-authorized` — exchange the code and run the import.
#[handler]
#[tracing::instrument(skip(session))]
pub async fn authorized(
    session: &Session,
    Query(query): Query<AuthorizedQuery>,
) -> Result<Response, anyhow::Error> {
    // Single use: whatever happens next, this handshake's state is spent.
    let expected: Option<String> = session.get(STATE_KEY);
    session.remove(STATE_KEY);

    if expected.is_none() || expected.as_deref() != Some(query.state.as_str()) {
        return Ok(message(
            "This authorization did not come from a request made here. \
             Start again from the authorize page.",
        ));
    }

    if !query.error.is_empty() {
        return Ok(message(&format!(
            "Google declined the authorization: {}",
            html_escape(&query.error)
        )));
    }

    if query.code.is_empty() {
        return Ok(message("Google returned no authorization code."));
    }

    let client = reqwest::Client::new();

    let token: TokenResponse = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", CLIENT_ID),
            ("client_secret", GOOGLE_AUTH_SECRET),
            ("code", query.code.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", REDIRECT_URI),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let sheet: SheetValues = client
        .get(format!(
            "https://sheets.googleapis.com/v4/spreadsheets/{LIBRARY_SHEET}/values/{LIBRARY_RANGE}"
        ))
        .bearer_auth(&token.access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // The first row is the header.
    let rows: Vec<Vec<String>> = sheet.values.into_iter().skip(1).collect();

    let stream = futures::stream::unfold(
        Some((0usize, rows, Totals::default())),
        |state| async move {
            let (index, rows, mut totals) = state?;

            if index == 0 {
                return Some((
                    Ok::<_, std::io::Error>(String::from(
                        "<!doctype html><html><head><meta charset=\"utf-8\">\
                     <title>Google Integrations</title>\
                     <style>body{font-family:sans-serif;margin:2rem;max-width:720px;}\
                     .miss{color:#b00;}.skip{color:#888;}</style></head><body>\
                     <p><a href=\"/staff/google-authorize\">&laquo; back</a></p>\
                     <h1>Syncing libraries</h1><p>",
                    )),
                    Some((1, rows, totals)),
                ));
            }

            let Some(row) = rows.get(index - 1) else {
                return Some((Ok(totals.summary()), None));
            };

            let line = import_row(row, &mut totals).await;

            Some((Ok(line), Some((index + 1, rows, totals))))
        },
    );

    Ok(Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(poem::Body::from_bytes_stream(stream)))
}

/// What the import did, counted as it goes.
#[derive(Debug, Default, Clone, Copy)]
struct Totals {
    added: u32,
    already_known: u32,
    unlocated: u32,
}

impl Totals {
    fn summary(&self) -> String {
        format!(
            "</p><p><strong>Done.</strong> {} added, {} already in the directory, \
             {} added without a location because the address could not be resolved.</p>\
             <p>Correct those addresses in the sheet and run the import again — \
             a library already in the directory is left alone, so nothing is \
             overwritten.</p></body></html>",
            self.added, self.already_known, self.unlocated,
        )
    }
}

/// Import one sheet row, returning the progress line to stream for it.
async fn import_row(row: &[String], totals: &mut Totals) -> String {
    let cell = |index: usize| row.get(index).map(String::as_str).unwrap_or("").trim();

    let name = cell(0);

    if name.is_empty() {
        return String::new();
    }

    let address = cell(1);
    let city = cell(2);
    let state = cell(3);
    let zipcode = cell(4);

    // Full address first; a street that the geocoder does not know falls back
    // to the town it is in, which is accurate enough for a map pin.
    let point = match geocode_parts(&[address, city, state, zipcode]).await {
        Some(point) => Some(point),
        None => geocode_parts(&[city, state, zipcode]).await,
    };

    if point.is_none() {
        totals.unlocated += 1;
    }

    let entry = LibraryDirectoryEntry {
        name: name.to_string(),
        address: address.to_string(),
        city: city.to_string(),
        state: state.to_string(),
        zipcode: zipcode.to_string(),
        org_type: cell(5).to_string(),
        url: cell(6).to_string(),
        point: point.clone().unwrap_or_default(),
        recorded: String::new(),
    };

    let recorded: Result<LibraryDirectoryRecordResponse, _> =
        request("user.library.directory_record", entry).await;

    match recorded {
        Ok(response) if response.added => {
            totals.added += 1;
            if point.is_some() {
                format!("{}<br>", html_escape(name))
            } else {
                format!(
                    "<span class=\"miss\">{} — added without a location</span><br>",
                    html_escape(name)
                )
            }
        }
        Ok(_) => {
            totals.already_known += 1;
            // Already-known rows are the bulk of every run after the first;
            // they are counted but not listed, or the page would be a wall of
            // names staff have seen before.
            String::new()
        }
        Err(error) => {
            tracing::error!(%error, library = name, "library directory import failed");
            format!(
                "<span class=\"miss\">{} — could not be stored</span><br>",
                html_escape(name)
            )
        }
    }
}

fn message(body: &str) -> Response {
    Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(format!(
            "<!doctype html><html><head><meta charset=\"utf-8\">\
             <title>Google Integrations</title>\
             <style>body{{font-family:sans-serif;margin:2rem;max-width:560px;}}</style>\
             </head><body><p>{body}</p>\
             <p><a href=\"/staff/google-authorize\">&laquo; back</a></p></body></html>"
        ))
}
