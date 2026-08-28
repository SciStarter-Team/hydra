//! GET `/staff/events/events.html` and `/staff/events/events.csv` — the event
//! organizer contact export, ported from orrery's `src/staff/events.rs`.
//!
//! Staff run this before Citizen Science Month to reach the people behind the
//! events happening in a window: one row per published event that meets in the
//! window, carrying the manager and contact-person details alongside the event's
//! own description and every scheduled instance.
//!
//! orrery stores each event's schedule as ten numbered attribute pairs
//! (`begin`/`end`, `begin1`/`end1`, …) and its export has a column per slot.
//! hydra keeps instances in a table with no such ceiling, so the CSV grows its
//! `begin N`/`end N` columns to fit the busiest event in the result — an event
//! with eleven meetings is exported whole rather than silently truncated, and a
//! window where nothing repeats produces just `begin 1`/`end 1`.

use common::shared::db;
use jiff::ToSpan;
use poem::{Response, handler, web::Query};
use serde::Deserialize;
use sqlx::Row;

use super::csv_field;

/// The reporting window, as the form submits it. Both are `YYYY-MM-DD`.
#[derive(Debug, Deserialize)]
pub struct RangeQuery {
    #[serde(default)]
    begin: String,
    #[serde(default)]
    end: String,
}

/// One event, with its schedule and the two contact roles staff write to.
struct EventRow {
    title: String,
    tags: String,
    event_type: String,
    manager_name: String,
    manager_affiliation: String,
    manager_email: String,
    manager_phone: String,
    manager_address: String,
    contact_name: String,
    contact_affiliation: String,
    contact_email: String,
    contact_phone: String,
    contact_address: String,
    presenter: String,
    url: String,
    location_type: String,
    entirely_online: bool,
    location_description: String,
    address: String,
    city: String,
    state: String,
    country: String,
    zip: String,
    /// Every instance of this event, in time order, as `(begin, end)`.
    instances: Vec<(String, String)>,
}

/// One contact row, with the nullable columns already defaulted, so the export
/// does not have to think about the difference between "no phone recorded" and
/// "empty phone recorded".
#[derive(Debug, Default, Clone)]
struct Contact {
    role: String,
    name: String,
    affiliation: String,
    email: String,
    phone: String,
    address: String,
}

const INSTANCES_SQL: &str = r#"SELECT event_id, "begin", "end" FROM event_instances
                               WHERE event_id IN"#;
const TAGS_SQL: &str = "SELECT event_id, tag FROM event_tags WHERE event_id IN";
const CONTACTS_SQL: &str = r#"SELECT event_id, role, name, affiliation, email, phone, address
                              FROM event_contacts WHERE event_id IN"#;

/// Run one `… WHERE event_id IN (…)` query and group its rows by event.
///
/// `extract` turns a row into the event id it belongs to and the value to file
/// under it. Grouping here rather than in each caller keeps the three lookups
/// to one shape.
///
/// Ordering is by event then by the query's remaining columns, so instances
/// come back in time order and tags alphabetically without a sort per event.
async fn collect<T, F>(
    ids: &[String],
    sql: &str,
    extract: F,
) -> Result<std::collections::HashMap<String, Vec<T>>, anyhow::Error>
where
    F: Fn(&sqlx::sqlite::SqliteRow) -> (String, T),
{
    use sqlx::QueryBuilder;

    let mut grouped: std::collections::HashMap<String, Vec<T>> = std::collections::HashMap::new();

    if ids.is_empty() {
        return Ok(grouped);
    }

    // SQLite caps a statement at 999 bound parameters by default, and a busy
    // month can hold more events than that.
    for chunk in ids.chunks(900) {
        let mut builder = QueryBuilder::new(sql);
        builder.push(" (");
        let mut separated = builder.separated(", ");
        for id in chunk {
            separated.push_bind(id);
        }
        builder.push(") ORDER BY 1, 2");

        for row in builder.build().fetch_all(db()).await? {
            let (id, value) = extract(&row);
            grouped.entry(id).or_default().push(value);
        }
    }

    Ok(grouped)
}

/// The fixed columns, in the order orrery's export puts them. The per-instance
/// `begin N`/`end N` pairs follow.
const FIXED_COLUMNS: &[&str] = &[
    "title",
    "tags",
    "event_type",
    "manager_name",
    "manager_affiliation",
    "manager_email",
    "manager_phone",
    "manager_address",
    "contact_name",
    "contact_affiliation",
    "contact_email",
    "contact_phone",
    "contact_address",
    "presenter",
    "url",
    "location_type",
    "entirely_online",
    "location_description",
    "address",
    "city",
    "state",
    "country",
    "zip",
];

/// GET `/staff/events/events.html` — the date-range form.
#[handler]
pub async fn form() -> Response {
    let html = "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>SciStarter Events Export</title>\
         <style>body{font-family:sans-serif;margin:2rem;}\
         label{display:block;margin:.5rem 0;}</style></head>\
         <body><h1>Events export</h1>\
         <p>Published events meeting between the two dates, with organizer contact details.</p>\
         <form method=\"get\" action=\"/staff/events/events.csv\">\
         <label>Begin date <input type=\"date\" name=\"begin\" required></label>\
         <label>End date <input type=\"date\" name=\"end\" required></label>\
         <p><button type=\"submit\">Export CSV</button></p>\
         </form></body></html>";

    Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(html)
}

/// GET `/staff/events/events.csv` — the export itself.
///
/// An event qualifies when any one of its instances overlaps the window, which
/// is `end > begin_of_window AND begin < end_of_window` — the same test orrery
/// applies to each of its ten slots in turn.
#[handler]
#[tracing::instrument]
pub async fn export_csv(Query(range): Query<RangeQuery>) -> Result<Response, anyhow::Error> {
    if range.begin.is_empty() || range.end.is_empty() {
        return Ok(Response::builder()
            .status(poem::http::StatusCode::SEE_OTHER)
            .header("Location", "/staff/events/events.html")
            .finish());
    }

    // The form submits bare dates; instances are stored as timestamps, so the
    // window runs from the start of the begin date to the start of the day
    // after the end date, making both endpoints inclusive days.
    let end_date: jiff::civil::Date = range
        .end
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid end date: {e}"))?;
    let begin = format!("{}T00:00:00Z", range.begin);
    let end = format!("{}T00:00:00Z", end_date.checked_add(1.day())?);

    struct Base {
        event_id: String,
        name: String,
        presenter: String,
        url: String,
        event_type: String,
        location_type: String,
        entirely_online: bool,
        location_description: String,
        address: String,
        city: String,
        state: String,
        country: String,
        zip: String,
    }

    let rows = sqlx::query_as!(
        Base,
        r#"
        SELECT
            ev.event_id AS "event_id!",
            ev.name AS "name!",
            ev.presenter AS "presenter!",
            ev.url AS "url!",
            ev.event_type AS "event_type!",
            ev.location_type AS "location_type!",
            ev.entirely_online AS "entirely_online!: bool",
            ev.location_description AS "location_description!",
            ev.address AS "address!",
            ev.city AS "city!",
            ev.state AS "state!",
            ev.country AS "country!",
            ev.zip AS "zip!"
        FROM events ev
        JOIN entities e ON e.target = ev.event_id AND e.kind = 'Event'
        WHERE e.publish = 1
          AND EXISTS (
              SELECT 1 FROM event_instances i
              WHERE i.event_id = ev.event_id AND i."end" > ?1 AND i."begin" < ?2
          )
        ORDER BY ev.name
        "#,
        begin,
        end,
    )
    .fetch_all(db())
    .await?;

    let ids: Vec<String> = rows.iter().map(|r| r.event_id.clone()).collect();

    // Three batched lookups rather than three per event: the export covers a
    // whole month of events, and a query per row per table made it the slowest
    // page staff had.
    let mut instances = collect(&ids, INSTANCES_SQL, |row| {
        (
            row.get::<String, _>(0),
            (row.get::<String, _>(1), row.get::<String, _>(2)),
        )
    })
    .await?;
    let mut tags = collect(&ids, TAGS_SQL, |row| {
        (row.get::<String, _>(0), row.get::<String, _>(1))
    })
    .await?;
    let mut contacts = collect(&ids, CONTACTS_SQL, |row| {
        (
            row.get::<String, _>(0),
            Contact {
                role: row.get(1),
                name: row.get::<Option<String>, _>(2).unwrap_or_default(),
                affiliation: row.get::<Option<String>, _>(3).unwrap_or_default(),
                email: row.get(4),
                phone: row.get::<Option<String>, _>(5).unwrap_or_default(),
                address: row.get::<Option<String>, _>(6).unwrap_or_default(),
            },
        )
    })
    .await?;

    let mut events = Vec::with_capacity(rows.len());

    for row in rows {
        let event_contacts = contacts.remove(&row.event_id).unwrap_or_default();

        // An event can carry several contacts in a role; the export takes the
        // first of each, as orrery's single-valued attributes did.
        let pick = |wanted: &str| {
            event_contacts
                .iter()
                .find(|c| c.role == wanted)
                .cloned()
                .unwrap_or_default()
        };

        let manager = pick("Manager");
        let contact = pick("ContactPerson");

        events.push(EventRow {
            title: row.name,
            tags: tags.remove(&row.event_id).unwrap_or_default().join("||"),
            event_type: row.event_type,
            manager_name: manager.name,
            manager_affiliation: manager.affiliation,
            manager_email: manager.email,
            manager_phone: manager.phone,
            manager_address: manager.address,
            contact_name: contact.name,
            contact_affiliation: contact.affiliation,
            contact_email: contact.email,
            contact_phone: contact.phone,
            contact_address: contact.address,
            presenter: row.presenter,
            url: row.url,
            location_type: row.location_type,
            entirely_online: row.entirely_online,
            location_description: row.location_description,
            address: row.address,
            city: row.city,
            state: row.state,
            country: row.country,
            zip: row.zip,
            instances: instances.remove(&row.event_id).unwrap_or_default(),
        });
    }

    let csv = render_csv(&events);

    Ok(Response::builder()
        .content_type("text/csv")
        .header(
            "Content-Disposition",
            format!(
                "attachment; filename=\"events_{}_{}.csv\"",
                range.begin, range.end
            ),
        )
        .body(csv))
}

/// Render the export, widening the schedule columns to the busiest event.
fn render_csv(events: &[EventRow]) -> String {
    let slots = events
        .iter()
        .map(|e| e.instances.len())
        .max()
        .unwrap_or(0)
        // Even an empty export carries one pair, so the header shape is
        // recognizable to whatever reads it.
        .max(1);

    let mut csv = FIXED_COLUMNS.join(",");
    for slot in 1..=slots {
        csv.push_str(&format!(",begin {slot},end {slot}"));
    }
    csv.push('\n');

    for event in events {
        let fields = [
            event.title.as_str(),
            event.tags.as_str(),
            event.event_type.as_str(),
            event.manager_name.as_str(),
            event.manager_affiliation.as_str(),
            event.manager_email.as_str(),
            event.manager_phone.as_str(),
            event.manager_address.as_str(),
            event.contact_name.as_str(),
            event.contact_affiliation.as_str(),
            event.contact_email.as_str(),
            event.contact_phone.as_str(),
            event.contact_address.as_str(),
            event.presenter.as_str(),
            event.url.as_str(),
            event.location_type.as_str(),
            if event.entirely_online {
                "true"
            } else {
                "false"
            },
            event.location_description.as_str(),
            event.address.as_str(),
            event.city.as_str(),
            event.state.as_str(),
            event.country.as_str(),
            event.zip.as_str(),
        ];

        let mut line: Vec<String> = fields.iter().map(|f| csv_field(f)).collect();

        for slot in 0..slots {
            let (begin, end) = event
                .instances
                .get(slot)
                .map(|(b, e)| (b.as_str(), e.as_str()))
                .unwrap_or(("", ""));
            line.push(csv_field(begin));
            line.push(csv_field(end));
        }

        csv.push_str(&line.join(","));
        csv.push('\n');
    }

    csv
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(title: &str, instances: &[(&str, &str)]) -> EventRow {
        EventRow {
            title: title.to_string(),
            tags: String::new(),
            event_type: String::new(),
            manager_name: String::new(),
            manager_affiliation: String::new(),
            manager_email: String::new(),
            manager_phone: String::new(),
            manager_address: String::new(),
            contact_name: String::new(),
            contact_affiliation: String::new(),
            contact_email: String::new(),
            contact_phone: String::new(),
            contact_address: String::new(),
            presenter: String::new(),
            url: String::new(),
            location_type: String::new(),
            entirely_online: false,
            location_description: String::new(),
            address: String::new(),
            city: String::new(),
            state: String::new(),
            country: String::new(),
            zip: String::new(),
            instances: instances
                .iter()
                .map(|(b, e)| (b.to_string(), e.to_string()))
                .collect(),
        }
    }

    #[test]
    fn header_carries_one_slot_when_nothing_repeats() {
        let csv = render_csv(&[event("One", &[("a", "b")])]);
        let header = csv.lines().next().expect("a header");

        assert!(header.ends_with(",begin 1,end 1"), "{header}");
    }

    #[test]
    fn header_widens_to_the_busiest_event() {
        let csv = render_csv(&[
            event("One", &[("a", "b")]),
            event("Two", &[("a", "b"), ("c", "d"), ("e", "f")]),
        ]);
        let header = csv.lines().next().expect("a header");

        assert!(
            header.ends_with(",begin 1,end 1,begin 2,end 2,begin 3,end 3"),
            "{header}"
        );
    }

    #[test]
    fn every_row_has_as_many_fields_as_the_header() {
        let csv = render_csv(&[
            event("One", &[("a", "b")]),
            event("Two", &[("a", "b"), ("c", "d")]),
        ]);

        let counts: Vec<usize> = csv.lines().map(|line| line.split(',').count()).collect();

        assert_eq!(counts.len(), 3);
        assert!(
            counts.iter().all(|c| *c == counts[0]),
            "ragged rows: {counts:?}"
        );
    }

    #[test]
    fn an_empty_export_still_has_a_header() {
        let csv = render_csv(&[]);

        assert_eq!(csv.lines().count(), 1);
        assert!(csv.starts_with("title,tags,"));
    }

    #[test]
    fn a_title_with_a_comma_is_quoted() {
        let csv = render_csv(&[event("Bugs, Bees and Beyond", &[("a", "b")])]);

        assert!(csv.contains("\"Bugs, Bees and Beyond\""), "{csv}");
    }
}
