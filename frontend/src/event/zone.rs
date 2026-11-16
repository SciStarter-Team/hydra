//! An event's own clocks: the time zone of its venue, which its dates are
//! shown and entered on, as orrery's event form shows them. Only an event held
//! in person at an address has a venue. Any other has no zone of its own --
//! one joined online, one held wherever each participant is, and one across a
//! region, which can span zones -- and its dates are shown on the viewer's
//! clocks instead, though it may carry a point (a region's centroid, or one
//! left from before it moved).

use anyhow::Error;
use common::shared::db;
use jiff::tz::TimeZone;

use crate::project::location::LocationMode;

/// The IANA name of the time zone at a coordinate, from tzf-rs's embedded
/// boundary data (empty when the lookup has no answer). The finder builds its
/// spatial index once, on first use.
pub fn name_at(latitude: f64, longitude: f64) -> String {
    static FINDER: std::sync::OnceLock<tzf_rs::DefaultFinder> = std::sync::OnceLock::new();
    FINDER
        .get_or_init(tzf_rs::DefaultFinder::new)
        .get_tz_name(longitude, latitude)
        .to_string()
}

/// The time zone at a coordinate, when there is one.
pub fn at(latitude: f64, longitude: f64) -> Option<TimeZone> {
    TimeZone::get(&name_at(latitude, longitude)).ok()
}

/// The time zone of the event `event_id`'s venue, from the point it is pinned
/// at; `None` for an event without a venue, one with no point, or one pinned
/// at 0,0, which marks a place nobody found rather than a place.
pub async fn of_event(event_id: &str) -> Result<Option<TimeZone>, Error> {
    let row = sqlx::query!(
        r#"SELECT ST_Y(point) AS "latitude: f64", ST_X(point) AS "longitude: f64",
                  location_type, entirely_online AS "entirely_online: bool"
           FROM events WHERE event_id = ?"#,
        event_id
    )
    .fetch_optional(db())
    .await?;

    Ok(row.and_then(|row| {
        of_venue(
            row.latitude,
            row.longitude,
            &row.location_type,
            row.entirely_online,
        )
    }))
}

/// The time zone of the venue of an event pinned at `latitude` and
/// `longitude`, with its `location_type` and whether it's `entirely_online`,
/// as [`of_event`] finds an event's.
pub fn of_venue(
    latitude: Option<f64>,
    longitude: Option<f64>,
    location_type: &str,
    entirely_online: bool,
) -> Option<TimeZone> {
    let (latitude, longitude) = (latitude?, longitude?);
    let held_at_a_venue = LocationMode::of_event(location_type, entirely_online)
        .is_some_and(LocationMode::at_address);
    if !held_at_a_venue || (latitude == 0.0 && longitude == 0.0) {
        return None;
    }
    at(latitude, longitude)
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    #[test]
    fn only_an_event_held_at_an_address_keeps_its_points_clocks() {
        use crate::project::location::{ANYWHERE, AT_AN_ADDRESS, IN_A_RANGE};

        let philadelphia = |location_type, entirely_online| {
            of_venue(
                Some(39.9527),
                Some(-75.1635),
                location_type,
                entirely_online,
            )
            .and_then(|tz| tz.iana_name().map(str::to_string))
        };
        check!(philadelphia(AT_AN_ADDRESS, false) == Some("America/New_York".to_string()));
        // A region's centroid, and a point left on an event that has since
        // gone online or anywhere, are no venue.
        check!(philadelphia(IN_A_RANGE, false) == None);
        check!(philadelphia(ANYWHERE, false) == None);
        check!(philadelphia(AT_AN_ADDRESS, true) == None);
        check!(of_venue(Some(0.0), Some(0.0), AT_AN_ADDRESS, false).is_none());
    }

    #[test]
    fn a_venue_has_the_zone_its_clocks_keep() {
        check!(name_at(39.9527, -75.1635) == "America/New_York");
        check!(
            at(41.8781, -87.6298).and_then(|tz| tz.iana_name().map(str::to_string))
                == Some("America/Chicago".to_string())
        );
    }
}
