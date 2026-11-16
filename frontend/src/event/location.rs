//! Where an event is held, settled all at once from the answer to "Is it in
//! person or online?", as orrery's event form settles it. The parts depend on
//! each other -- an online event has no address, no kinds of venue and no
//! location note, and is of the Online type; only an event held at an
//! address has a venue -- so they are saved together, never one at a time.

use std::collections::BTreeSet;

use anyhow::Error;
use common::{message::location::Point, shared::db, transaction::Immediate};
use ulid::Ulid;

use crate::{
    choice::{ChoiceList, ChoiceValue, OrreryValue},
    event::all::EventAll,
    project::location::LocationMode,
};

/// An event's location, as it is saved.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct EventLocation {
    pub location_type: String,
    pub entirely_online: bool,
    pub location_name: String,
    pub address: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub country: String,
    /// The area a region covers, or a note like "meet at the north
    /// entrance"; an online event isn't held anywhere, and has none.
    pub location_description: String,
    pub url: String,
    /// Where the event is pinned, as GeoJSON: a picked place's point. `None`
    /// leaves the save to place the event at its address, or in the middle of
    /// its region, or nowhere.
    pub point: Option<String>,
    pub regions: Option<String>,
    /// The kinds of venue (slugs) an event held at an address is held at.
    pub venues: BTreeSet<String>,
}

/// A location as a form submits it.
#[derive(Debug, Clone, Copy)]
pub struct LocationInput<'a> {
    pub mode: LocationMode,
    pub location_name: &'a str,
    pub address: &'a str,
    pub city: &'a str,
    pub state: &'a str,
    pub zip: &'a str,
    pub country: &'a str,
    pub latitude: &'a str,
    pub longitude: &'a str,
    pub location_description: &'a str,
    pub url: &'a str,
    pub regions: &'a str,
    pub venues: &'a [String],
}

/// The address an event was saved at, and where it was pinned.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SavedAddress {
    pub parts: [String; 5],
    /// The latitude and longitude it was pinned at.
    pub coordinates: Option<(f64, f64)>,
}

impl SavedAddress {
    /// The address the event `event_id` is saved at, and where it's pinned.
    pub async fn of_event(event_id: &str) -> Result<Self, Error> {
        let row = sqlx::query!(
            r#"SELECT address, city, state, zip, country,
                      ST_Y(point) AS "latitude: f64", ST_X(point) AS "longitude: f64"
               FROM events WHERE event_id = ?"#,
            event_id
        )
        .fetch_one(db())
        .await?;
        Ok(SavedAddress {
            parts: [row.address, row.city, row.state, row.zip, row.country],
            coordinates: row.latitude.zip(row.longitude),
        })
    }

    /// Whether `latitude` and `longitude`, as a form submits them, are where
    /// the event was saved as pinned: the form shows them again unless a
    /// place is picked, or the address is typed over.
    fn pinned_at(&self, latitude: &str, longitude: &str) -> bool {
        let (Ok(latitude), Ok(longitude)) = (
            latitude.trim().parse::<f64>(),
            longitude.trim().parse::<f64>(),
        ) else {
            return false;
        };
        self.coordinates
            .is_some_and(|(saved_latitude, saved_longitude)| {
                (saved_latitude - latitude).abs() < 1e-7
                    && (saved_longitude - longitude).abs() < 1e-7
            })
    }
}

impl EventLocation {
    /// Settle `input` as orrery's event form settles its location. A picked
    /// place pins the point; an address typed or corrected by hand doesn't,
    /// and the save geocodes it instead. `saved` is the address the event was
    /// saved at, if any: a point that was only ever the saved address's is
    /// dropped when the address changes, even if the form kept it.
    pub fn settle(input: LocationInput<'_>, saved: Option<&SavedAddress>) -> Self {
        let mode = input.mode;
        let at_address = mode.at_address();
        let part = |value: &str| {
            if at_address {
                value.trim().to_string()
            } else {
                String::new()
            }
        };
        let address = [
            part(input.address),
            part(input.city),
            part(input.state),
            part(input.zip),
            part(input.country),
        ];

        let point = picked_point(input.latitude, input.longitude)
            .filter(|_| at_address)
            .filter(|_| match saved {
                Some(saved) => {
                    saved.parts == address || !saved.pinned_at(input.latitude, input.longitude)
                }
                None => true,
            });
        let regions = input.regions.trim();

        let [address, city, state, zip, country] = address;
        EventLocation {
            location_type: mode.event_location_type().to_string(),
            entirely_online: mode == LocationMode::Online,
            location_name: part(input.location_name),
            address,
            city,
            state,
            zip,
            country,
            location_description: if mode == LocationMode::Online {
                String::new()
            } else {
                input.location_description.trim().to_string()
            },
            url: input.url.trim().to_string(),
            point,
            regions: (mode == LocationMode::Region && !regions.is_empty())
                .then(|| regions.to_string()),
            venues: if at_address {
                input.venues.iter().cloned().collect()
            } else {
                BTreeSet::new()
            },
        }
    }

    /// Set `event`'s location to this one; its kinds of venue are saved with
    /// the rest of its event types.
    pub fn apply_to(self, event: &mut EventAll) {
        event.location_type = self.location_type;
        event.entirely_online = self.entirely_online;
        event.location_name = self.location_name;
        event.address = self.address;
        event.city = self.city;
        event.state = self.state;
        event.zip = self.zip;
        event.country = self.country;
        event.location_description = self.location_description;
        event.url = self.url;
        event.point = self.point;
        event.regions = self.regions;
    }

    /// Whether the event is held at an address, any part of which there is.
    pub fn has_address(&self) -> bool {
        [
            &self.address,
            &self.city,
            &self.state,
            &self.zip,
            &self.country,
        ]
        .iter()
        .any(|part| !part.is_empty())
    }

    /// The saved location of the event `event_id`.
    pub async fn read(event_id: Ulid) -> Result<Self, Error> {
        let id = event_id.to_string();
        let row = sqlx::query!(
            r#"SELECT location_type, entirely_online AS "entirely_online: bool",
                      location_name, address, city, state, zip, country,
                      location_description, url,
                      ST_Y(point) AS "latitude: f64", ST_X(point) AS "longitude: f64",
                      AsGeoJSON(regions) AS "regions: String"
               FROM events WHERE event_id = ?"#,
            id
        )
        .fetch_one(db())
        .await?;
        let venues = ChoiceValue::of_target(ChoiceList::EventVenue, &id).await?;

        Ok(EventLocation {
            location_type: row.location_type,
            entirely_online: row.entirely_online,
            location_name: row.location_name,
            address: row.address,
            city: row.city,
            state: row.state,
            zip: row.zip,
            country: row.country,
            location_description: row.location_description,
            url: row.url,
            // Written as a settled location writes it, so that one left as it
            // was compares equal.
            point: row
                .latitude
                .zip(row.longitude)
                .map(|(latitude, longitude)| point_at(latitude, longitude)),
            regions: row.regions,
            venues: venues.into_iter().collect(),
        })
    }

    /// Save the location of the event `event_id`, and its kinds of venue and
    /// whether it is of the Online type with it, in one transaction. Then
    /// place it where [`place_event`] does.
    pub async fn save(&self, event_id: Ulid) -> Result<(), Error> {
        let id = event_id.to_string();
        let online = OrreryValue::EVENT_ONLINE.find().await?;

        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;
        sqlx::query!(
            r#"UPDATE events SET
                 location_type = ?1, entirely_online = ?2,
                 also_online = also_online AND NOT ?2,
                 location_name = ?3, address = ?4, city = ?5, state = ?6, zip = ?7,
                 country = ?8, location_description = ?9, url = ?10,
                 point = CASE WHEN ?11 IS NULL THEN NULL
                              ELSE SetSRID(GeomFromGeoJSON(?11), 4326) END,
                 regions = CASE WHEN ?12 IS NULL THEN NULL
                                ELSE SetSRID(GeomFromGeoJSON(?12), 4326) END
               WHERE event_id = ?13"#,
            self.location_type,
            self.entirely_online,
            self.location_name,
            self.address,
            self.city,
            self.state,
            self.zip,
            self.country,
            self.location_description,
            self.url,
            self.point,
            self.regions,
            id,
        )
        .execute(&mut tx)
        .await?;

        crate::choice::assign(&mut tx, &id, ChoiceList::EventVenue, &self.venues).await?;
        if let Some(online) = &online {
            let mut picked = crate::choice::assigned(&mut tx, &id, online.list).await?;
            settle_online(&mut picked, online, self.entirely_online);
            crate::choice::assign(&mut tx, &id, online.list, &picked).await?;
        }
        tx.commit().await?;

        place_event(
            event_id,
            self.point.is_some(),
            self.regions.is_some(),
            self.has_address(),
        )
        .await;
        Ok(())
    }
}

/// Tick or untick `online`, the type saying an event is held online, among
/// the `picked` slugs of its list, as the event is or isn't: it comes from
/// the location question alone.
pub fn settle_online(picked: &mut BTreeSet<String>, online: &ChoiceValue, entirely_online: bool) {
    if entirely_online {
        picked.insert(online.slug.clone());
    } else {
        picked.remove(&online.slug);
    }
}

/// Place the event `event_id`, just saved, when its save didn't pin it:
/// in the middle of its region, or at its address, or nowhere. Then work out
/// again which regions it falls in, which both its point and its area decide.
/// A failure leaves the event unplaced, or the cache stale until the next
/// rebuild, rather than failing a save that has already been committed.
pub async fn place_event(event_id: Ulid, pinned: bool, has_regions: bool, has_address: bool) {
    let id = event_id.to_string();
    if !pinned {
        if has_regions {
            if let Err(e) = sqlx::query!(
                r#"UPDATE events SET point = ST_Centroid(regions)
                   WHERE event_id = ? AND point IS NULL AND regions IS NOT NULL"#,
                id
            )
            .execute(db())
            .await
            {
                eprintln!("Region-centroid point for event {event_id} failed: {e}");
            }
        } else if has_address {
            if let Err(e) = crate::geocode::geocode_and_update_event_point(event_id).await {
                eprintln!("Auto-geocode for event {event_id} failed: {e}");
            }
        }
    }

    if let Err(e) = crate::region::refresh_event(&id).await {
        eprintln!("Region refresh for event {event_id} failed: {e}");
    }
}

/// The GeoJSON point a place picked in the place search pins, from its
/// latitude and longitude; `None` when there are none, or they're no place.
pub fn picked_point(latitude: &str, longitude: &str) -> Option<String> {
    let latitude: f64 = latitude.trim().parse().ok()?;
    let longitude: f64 = longitude.trim().parse().ok()?;
    if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
        return None;
    }
    Some(point_at(latitude, longitude))
}

/// The GeoJSON point at `latitude` and `longitude`.
fn point_at(latitude: f64, longitude: f64) -> String {
    Point {
        latitude,
        longitude,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    fn input(mode: LocationMode) -> LocationInput<'static> {
        LocationInput {
            mode,
            location_name: "Franklin Institute",
            address: "222 N 20th St",
            city: "Philadelphia",
            state: "PA",
            zip: "19103",
            country: "USA",
            latitude: "39.9582",
            longitude: "-75.1731",
            location_description: "Meet at the north entrance",
            url: "https://example.org/bugs",
            regions: r#"{"type":"MultiPolygon","coordinates":[]}"#,
            venues: &[],
        }
    }

    #[test]
    fn an_online_event_is_held_nowhere() {
        let venues = ["museum".to_string()];
        let location = EventLocation::settle(
            LocationInput {
                venues: &venues,
                ..input(LocationMode::Online)
            },
            None,
        );
        check!(location.entirely_online);
        check!(location.location_type == crate::project::location::ANYWHERE);
        check!(!location.has_address());
        check!(location.location_name.is_empty());
        check!(location.location_description.is_empty());
        check!(location.point.is_none());
        check!(location.regions.is_none());
        check!(location.venues.is_empty());
        check!(location.url == "https://example.org/bugs");
    }

    #[test]
    fn only_an_event_at_an_address_has_a_venue() {
        let venues = ["museum".to_string()];
        let at = EventLocation::settle(
            LocationInput {
                venues: &venues,
                ..input(LocationMode::InPerson)
            },
            None,
        );
        check!(at.venues.contains("museum"));
        check!(at.point.is_some());
        check!(at.location_description == "Meet at the north entrance");
        check!(at.regions.is_none());

        let anywhere = EventLocation::settle(
            LocationInput {
                venues: &venues,
                ..input(LocationMode::Anywhere)
            },
            None,
        );
        check!(anywhere.venues.is_empty());
        check!(!anywhere.has_address());
        check!(!anywhere.entirely_online);
        check!(anywhere.location_type == crate::project::location::ANYWHERE);
    }

    #[test]
    fn a_region_is_drawn_and_described() {
        let region = EventLocation::settle(input(LocationMode::Region), None);
        check!(region.regions.is_some());
        check!(region.location_type == crate::project::location::IN_A_RANGE);
        check!(region.location_description == "Meet at the north entrance");
        check!(region.point.is_none());
    }

    #[test]
    fn a_changed_address_drops_the_old_addresss_point() {
        let saved_point = picked_point("39.9582", "-75.1731");
        let saved = SavedAddress {
            parts: ["1 Old Rd", "Philadelphia", "PA", "19103", "USA"].map(str::to_string),
            coordinates: Some((39.9582, -75.1731)),
        };
        let moved = EventLocation::settle(input(LocationMode::InPerson), Some(&saved));
        check!(moved.point.is_none());

        let kept = SavedAddress {
            parts: ["222 N 20th St", "Philadelphia", "PA", "19103", "USA"].map(str::to_string),
            ..saved.clone()
        };
        check!(
            EventLocation::settle(input(LocationMode::InPerson), Some(&kept)).point == saved_point
        );

        // A newly picked place pins its own point, whatever was saved.
        let picked = SavedAddress {
            coordinates: Some((40.0, -75.0)),
            ..saved
        };
        check!(
            EventLocation::settle(input(LocationMode::InPerson), Some(&picked)).point
                == saved_point
        );
    }
}
