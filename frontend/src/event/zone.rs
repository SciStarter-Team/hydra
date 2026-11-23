//! An event's own clocks: the time zone of its venue, which its dates are
//! shown and entered on, as orrery's event form shows them. Only an event held
//! in person at an address has a venue. Any other has no zone of its own --
//! one joined online, one held wherever each participant is, and one across a
//! region, which can span zones -- and its dates are shown on the viewer's
//! clocks instead, though it may carry a point (a region's centroid, or one
//! left from before it moved). A venue's zone is that of the point it is
//! pinned at, or, for one never pinned, of where its address is.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use anyhow::Error;
use common::shared::db;
use jiff::tz::TimeZone;

use crate::{lru_cache::LruCache, project::location::LocationMode};

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

/// How long a zone found from an address is kept, and how long an address
/// is taken to have none once its lookup found none (or failed), before it
/// is asked again.
const FOUND_FOR: Duration = Duration::from_secs(24 * 60 * 60);
const MISSED_FOR: Duration = Duration::from_secs(60 * 60);

/// The most addresses whose zones each of those remembers.
const ADDRESSES_KEPT: usize = 500;

static FOUND: LazyLock<Mutex<LruCache<String>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(ADDRESSES_KEPT, FOUND_FOR)));
static MISSED: LazyLock<Mutex<LruCache<()>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(ADDRESSES_KEPT, MISSED_FOR)));

/// Where an event is held, as its zone is found from it.
#[derive(Debug, Clone, Copy)]
pub struct EventVenue<'a> {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub location_type: &'a str,
    pub entirely_online: bool,
    pub address: &'a str,
    pub city: &'a str,
    pub state: &'a str,
    pub zip: &'a str,
    pub country: &'a str,
}

impl EventVenue<'_> {
    /// The zone of the venue, as orrery's `EventVenue` finds it: for an event
    /// held at an address, the zone of the point it is pinned at, or, with
    /// none, of where its address is, when that gives a city or a zip; and
    /// for any other event, none.
    pub async fn zone(&self) -> Option<TimeZone> {
        if !held_at_a_venue(self.location_type, self.entirely_online) {
            return None;
        }
        if let Some(zone) = point_zone(self.latitude, self.longitude) {
            return Some(zone);
        }
        if self.city.trim().is_empty() && self.zip.trim().is_empty() {
            return None;
        }
        address_zone(&[self.address, self.city, self.state, self.zip, self.country]).await
    }
}

/// The time zone of the event `event_id`'s venue ([`EventVenue::zone`]);
/// `None` for an event without one, or that isn't saved.
pub async fn of_event(event_id: &str) -> Result<Option<TimeZone>, Error> {
    Ok(of_events([event_id]).await?.remove(event_id))
}

/// The time zones of the venues of the events `event_ids`
/// ([`EventVenue::zone`]), by event id, read together, as a listing of many
/// events finds them; an event without one is left out. A listing names an
/// event's times on the same clocks its page does.
pub async fn of_events<I>(event_ids: I) -> Result<HashMap<String, TimeZone>, Error>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT event_id, ST_Y(point), ST_X(point), location_type, entirely_online, \
                address, city, state, zip, country \
         FROM events WHERE event_id IN (",
    );
    let mut ids = query.separated(", ");
    let mut any = false;
    for id in event_ids {
        ids.push_bind(id.as_ref().to_string());
        any = true;
    }
    if !any {
        return Ok(HashMap::new());
    }
    query.push(")");

    type Row = (
        String,
        Option<f64>,
        Option<f64>,
        String,
        bool,
        String,
        String,
        String,
        String,
        String,
    );
    let rows = query.build_query_as::<Row>().fetch_all(db()).await?;

    let mut zones = HashMap::new();
    for (
        event_id,
        latitude,
        longitude,
        location_type,
        entirely_online,
        address,
        city,
        state,
        zip,
        country,
    ) in rows
    {
        let venue = EventVenue {
            latitude,
            longitude,
            location_type: &location_type,
            entirely_online,
            address: &address,
            city: &city,
            state: &state,
            zip: &zip,
            country: &country,
        };
        if let Some(zone) = venue.zone().await {
            zones.insert(event_id, zone);
        }
    }
    Ok(zones)
}

/// Whether an event is held at a venue: in person, at an address.
fn held_at_a_venue(location_type: &str, entirely_online: bool) -> bool {
    LocationMode::of_event(location_type, entirely_online).is_some_and(LocationMode::at_address)
}

/// The zone at a point; none for no point, or one at 0,0, which marks a
/// place nobody found rather than a place.
fn point_zone(latitude: Option<f64>, longitude: Option<f64>) -> Option<TimeZone> {
    let (latitude, longitude) = (latitude?, longitude?);
    if latitude == 0.0 && longitude == 0.0 {
        return None;
    }
    at(latitude, longitude)
}

/// The zone where the address made of `parts` is, remembered for a while
/// either way.
async fn address_zone(parts: &[&str]) -> Option<TimeZone> {
    let key = parts
        .iter()
        .map(|part| part.trim().to_lowercase())
        .collect::<Vec<_>>()
        .join("|");
    if let Some(name) = FOUND.lock().expect("zone cache poisoned").get(&key) {
        return TimeZone::get(&name).ok();
    }
    if MISSED
        .lock()
        .expect("zone cache poisoned")
        .get(&key)
        .is_some()
    {
        return None;
    }

    let zone = crate::geocode::locate(parts)
        .await
        .and_then(|point| at(point.latitude, point.longitude));
    match zone.as_ref().and_then(TimeZone::iana_name) {
        Some(name) => FOUND
            .lock()
            .expect("zone cache poisoned")
            .insert(key, name.to_string()),
        None => MISSED.lock().expect("zone cache poisoned").insert(key, ()),
    }
    zone
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    #[tokio::test]
    async fn only_an_event_held_at_an_address_keeps_its_points_clocks() {
        use crate::project::location::{ANYWHERE, AT_AN_ADDRESS, IN_A_RANGE};

        let zone = |latitude, location_type, entirely_online| async move {
            EventVenue {
                latitude: Some(latitude),
                longitude: Some(if latitude == 0.0 { 0.0 } else { -75.1635 }),
                location_type,
                entirely_online,
                address: "",
                city: "",
                state: "",
                zip: "",
                country: "",
            }
            .zone()
            .await
            .and_then(|tz| tz.iana_name().map(str::to_string))
        };
        check!(zone(39.9527, AT_AN_ADDRESS, false).await == Some("America/New_York".to_string()));
        // A region's centroid, and a point left on an event that has since
        // gone online or anywhere, are no venue.
        check!(zone(39.9527, IN_A_RANGE, false).await == None);
        check!(zone(39.9527, ANYWHERE, false).await == None);
        check!(zone(39.9527, AT_AN_ADDRESS, true).await == None);
        check!(zone(0.0, AT_AN_ADDRESS, false).await == None);
    }

    #[test]
    fn a_venue_has_the_zone_its_clocks_keep() {
        check!(name_at(39.9527, -75.1635) == "America/New_York");
        check!(
            at(41.8781, -87.6298).and_then(|tz| tz.iana_name().map(str::to_string))
                == Some("America/Chicago".to_string())
        );
    }

    #[tokio::test]
    async fn a_venue_is_zoned_by_its_point_before_its_address() {
        use crate::project::location::{AT_AN_ADDRESS, IN_A_RANGE};

        let venue = |latitude, longitude, location_type, city| EventVenue {
            latitude,
            longitude,
            location_type,
            entirely_online: false,
            address: "",
            city,
            state: "",
            zip: "",
            country: "",
        };
        let name = |zone: Option<TimeZone>| zone.and_then(|tz| tz.iana_name().map(str::to_string));

        // The point is the venue's, whatever the address says.
        check!(
            name(
                venue(Some(39.9527), Some(-75.1635), AT_AN_ADDRESS, "Chicago")
                    .zone()
                    .await
            ) == Some("America/New_York".to_string())
        );
        // A region is no venue, nor an address with neither city nor zip.
        check!(
            venue(Some(39.9527), Some(-75.1635), IN_A_RANGE, "Chicago")
                .zone()
                .await
                .is_none()
        );
        check!(venue(None, None, AT_AN_ADDRESS, "").zone().await.is_none());
    }
}
