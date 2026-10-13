use std::collections::HashSet;

use common::{
    insensitivestring::InsensitiveString, shared::db, timestring::TimeString,
    transaction::Immediate,
};
use ulid::Ulid;

use crate::{contact::Contact, peering::LoadStorePeered};

use super::{EventInstance, EventType, facebook::FacebookEvent};

/// Container for all event fields.
///
/// ⚠️  **PERFORMANCE WARNING**: This structure loads ALL 20+ event fields from the database.
/// Only use when you actually need most/all fields. For common operations, use direct SQL queries:
///
/// ```rust
/// // ❌ DON'T: Loading full object just for name and type
/// let event = EventAll::load(ulid).await?;
/// println!("{}: {}", event.name, event.event_type);
///
/// // ✅ DO: Direct query for specific fields
/// let result = sqlx::query!(
///     r#"SELECT "name", "event_type" FROM "events" WHERE "event_id" = ?"#,
///     ulid_str
/// ).fetch_one(db()).await?;
/// ```
///
/// This structure's primary purpose is for receiving data synchronizations. It should
/// not be used casually for other purposes, since there are more efficient options in most cases.
///
/// Note: The `tags_text` database column is NOT included here as it's
/// a computed field for FTS search optimization, not a real event field.
/// Tags are managed separately via the event_tags table.
#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EventAll {
    pub event_id: Ulid,
    pub created: TimeString,
    pub flagged: bool,
    pub admin_notes: String,
    pub welcomed: bool,
    pub page_creator_id: Ulid,        // NB was "owner"
    pub page_editor_ids: Box<[Ulid]>, // NB was "managers"
    pub event_last_editor: Option<Ulid>,
    pub event_last_edited: Option<TimeString>,
    pub scistarter_last_editor: Option<Ulid>,
    pub scistarter_last_edited: Option<TimeString>,
    pub name: String,
    pub tags: HashSet<InsensitiveString>,
    pub description: String,
    pub origin: String,
    pub contacts: Box<[Contact]>,
    pub presenter: String,
    pub url: String,
    pub image: String,
    pub image_credit: String,
    pub how_to_join: String,
    pub part_of: Option<Ulid>,
    pub location_type: String,
    pub entirely_online: bool,
    pub location_name: String,
    pub location_description: String,
    pub address: String,
    pub city: String,
    pub zip: String,
    pub state: String,
    pub country: String,
    pub regions: Option<String>,
    pub point: Option<String>,
    pub event_type: EventType,
    pub ticketing_begins: Option<TimeString>,
    pub outcomes_label: Option<String>,
    pub outcomes_link: Option<String>,
    pub circuit_id: Option<String>,
    pub instances: Box<[EventInstance]>,

    /// The ULID of the exchange the event was submitted through, if any.
    pub exchange: Option<String>,

    /// False while the event waits for the exchange it was submitted
    /// through to approve or reject it.
    pub reviewed: bool,

    /// What the organizer said about promoting the event on Facebook.
    pub facebook_event: FacebookEvent,
}

impl LoadStorePeered for EventAll {
    type DatabaseKey = Ulid;

    const GROUP: &str = "event";
    const SPECIFIC: &str = "all";

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), anyhow::Error> {
        let event_id = self.event_id.to_string();
        let part_of = self.part_of.map(|u| u.to_string());
        let page_creator_id = self.page_creator_id.to_string();
        let facebook_event = self.facebook_event.has();
        let facebook_event_url = self.facebook_event.url();
        let facebook_event_requested = self.facebook_event.create_requested();

        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

        sqlx::query!(
            r#"insert or replace into "events" (
              "event_id",
              "created",
              "flagged",
              "admin_notes",
              "welcomed",
              "page_creator_id",
              "name",
              "description",
              "origin",
              "presenter",
              "url",
              "image",
              "image_credit",
              "how_to_join",
              "part_of",
              "location_type",
              "entirely_online",
              "location_name",
              "location_description",
              "address",
              "city",
              "zip",
              "state",
              "country",
              "event_type",
              "ticketing_begins",
              "outcomes_label",
              "outcomes_link",
              "circuit_id",
              "regions",
              "point",
              "exchange",
              "reviewed",
              "facebook_event",
              "facebook_event_url",
              "facebook_event_requested"
            )
            values (
              ?1,
              ?2,
              ?3,
              ?4,
              ?5,
              ?6,
              ?7,
              ?8,
              ?9,
              ?10,
              ?11,
              ?12,
              ?13,
              ?14,
              ?15,
              ?16,
              ?17,
              ?18,
              ?19,
              ?20,
              ?21,
              ?22,
              ?23,
              ?24,
              ?25,
              ?26,
              ?27,
              ?28,
              ?29,
              case
                when ?30 is null then
                  null
                when substr(?30, 1, 1) == '{' and substr(?30, -1, 1) == '}' then
                  SetSRID(GeomFromGeoJSON(?30), 4326)
                else
                  GeomFromEWKT(?30)
              end,
              case
                when ?31 is null then
                  null
                when substr(?31, 1, 1) == '{' and substr(?31, -1, 1) == '}' then
                  SetSRID(GeomFromGeoJSON(?31), 4326)
                else
                  GeomFromEWKT(?31)
              end,
              ?32,
              ?33,
              ?34,
              ?35,
              ?36
            )"#,
            event_id,
            self.created,
            self.flagged,
            self.admin_notes,
            self.welcomed,
            page_creator_id,
            self.name,
            self.description,
            self.origin,
            self.presenter,
            self.url,
            self.image,
            self.image_credit,
            self.how_to_join,
            part_of,
            self.location_type,
            self.entirely_online,
            self.location_name,
            self.location_description,
            self.address,
            self.city,
            self.zip,
            self.state,
            self.country,
            self.event_type,
            self.ticketing_begins,
            self.outcomes_label,
            self.outcomes_link,
            self.circuit_id,
            self.regions,
            self.point,
            self.exchange,
            self.reviewed,
            facebook_event,
            facebook_event_url,
            facebook_event_requested,
        )
        .execute(&mut tx)
        .await?;

        // "page_editor_ids",
        sqlx::query!(
            r#"delete from event_page_editors where event_id = ?"#,
            event_id
        )
        .execute(&mut tx)
        .await?;

        for user_id in self.page_editor_ids.iter() {
            let user_id = user_id.to_string();
            sqlx::query!(
                r#"insert into event_page_editors (
                  "event_id",
                  "user_id"
                )
                values (
                  ?,
                  ?
                )"#,
                event_id,
                user_id,
            )
            .execute(&mut tx)
            .await?;
        }

        // "event_last_editor",
        // "event_last_edited",
        if let Some(last_editor) = self.event_last_editor {
            common::service::log_activity(
                self.event_last_edited.clone(),
                "user-edit",
                last_editor,
                self.event_id,
                Some("all".to_string()),
            )
            .await?;
        }

        // "scistarter_last_editor",
        // "scistarter_last_edited",
        if let Some(last_editor) = self.scistarter_last_editor {
            common::service::log_activity(
                self.scistarter_last_edited.clone(),
                "staff-edit",
                last_editor,
                self.event_id,
                Some("all".to_string()),
            )
            .await?;
        }

        // "tags",
        sqlx::query!(r#"delete from event_tags where event_id = ?"#, event_id)
            .execute(&mut tx)
            .await?;

        for val in self.tags.iter() {
            let val = val.as_ref();

            sqlx::query!(
                r#"insert into event_tags (
              "event_id",
              "tag"
            )
            values (
              ?,
              ?
            )"#,
                event_id,
                val,
            )
            .execute(&mut tx)
            .await?;
        }

        //pub contacts: Box<[Contact]>,
        sqlx::query!(r#"delete from event_contacts where event_id = ?"#, event_id)
            .execute(&mut tx)
            .await?;

        for contact in self.contacts.iter() {
            let contact_id = contact.contact_id.to_string();
            sqlx::query!(
                r#"insert into event_contacts (
                  "contact_id",
                  "event_id",
                  "role",
                  "name",
                  "affiliation",
                  "email",
                  "phone",
                  "address"
                )
                values (
                  ?,
                  ?,
                  ?,
                  ?,
                  ?,
                  ?,
                  ?,
                  ?
                )"#,
                contact_id,
                event_id,
                contact.role,
                contact.name,
                contact.affiliation,
                contact.email,
                contact.phone,
                contact.address,
            )
            .execute(&mut tx)
            .await?;
        }

        // pub instances: Box<[EventInstance]>,
        sqlx::query!(
            r#"delete from event_instances where event_id = ?"#,
            event_id
        )
        .execute(&mut tx)
        .await?;

        for val in self.instances.iter() {
            sqlx::query!(
                r#"insert into "event_instances" (
                  "event_id",
                  "begin",
                  "end"
                )
                values (
                  ?,
                  ?,
                  ?
                )"#,
                event_id,
                val.begin,
                val.end,
            )
            .execute(&mut tx)
            .await?;
        }

        tx.commit().await?;

        // Derive the point when it wasn't supplied: prefer the centroid of the
        // region polygon, fall back to geocoding the address/locality, and
        // otherwise leave it null (e.g. an entirely-online event).
        if self.point.is_none() {
            if self.regions.is_some() {
                let event_id = self.event_id.to_string();
                if let Err(e) = sqlx::query!(
                    r#"UPDATE events SET point = ST_Centroid(regions)
                       WHERE event_id = ? AND point IS NULL AND regions IS NOT NULL"#,
                    event_id
                )
                .execute(db())
                .await
                {
                    eprintln!(
                        "Region-centroid point for event {} failed: {e}",
                        self.event_id
                    );
                }
            } else if !self.city.is_empty() || !self.state.is_empty() || !self.country.is_empty() {
                if let Err(e) = crate::geocode::geocode_and_update_event_point(self.event_id).await
                {
                    eprintln!("Auto-geocode for event {} failed: {e}", self.event_id);
                }
            }
        }

        Ok(())
    }

    #[tracing::instrument]
    async fn read_from_database(event_id: Self::DatabaseKey) -> Result<Self, anyhow::Error> {
        let event_id_str = event_id.to_string();

        // Read main event data from events table
        let event_row = sqlx::query!(
            r#"SELECT
                event_id, created, flagged, admin_notes, welcomed,
                page_creator_id, name, description, origin, presenter, url,
                image, image_credit, how_to_join, part_of, location_type,
                entirely_online, location_name, location_description, address,
                city, zip, state, country, event_type as "event_type: EventType",
                ticketing_begins,
                outcomes_label, outcomes_link, circuit_id,
                AsGeoJSON(regions) as "regions: Option<String>",
                AsGeoJSON(point) as "point: Option<String>",
                exchange, reviewed as "reviewed: bool",
                facebook_event as "facebook_event: bool",
                facebook_event_url,
                facebook_event_requested as "facebook_event_requested: bool"
            FROM events WHERE event_id = ?"#,
            event_id_str
        )
        .fetch_one(db())
        .await?;

        // Read page_editor_ids
        let page_editor_ids = sqlx::query!(
            "SELECT user_id FROM event_page_editors WHERE event_id = ? ORDER BY user_id",
            event_id_str
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| {
            row.user_id
                .parse::<Ulid>()
                .map_err(|e| anyhow::anyhow!("Invalid editor ULID: {}", e))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();

        // Read tags
        let tags = sqlx::query!(
            "SELECT tag FROM event_tags WHERE event_id = ?",
            event_id_str
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| InsensitiveString::from(row.tag))
        .collect::<HashSet<_>>();

        // Read contacts
        let contact_rows = sqlx::query!(
            "SELECT contact_id, role, name, affiliation, email, phone, address FROM event_contacts WHERE event_id = ?",
            event_id_str
        )
        .fetch_all(db())
        .await?;

        let mut contacts = Vec::new();
        for contact_row in contact_rows {
            let contact_id = contact_row
                .contact_id
                .parse::<Ulid>()
                .map_err(|e| anyhow::anyhow!("Invalid contact ULID: {}", e))?;
            let role = match contact_row.role.as_str() {
                "Manager" => crate::contact::ContactRole::Manager,
                "ContactPerson" => crate::contact::ContactRole::ContactPerson,
                "Technical" => crate::contact::ContactRole::Technical,
                "Notification" => crate::contact::ContactRole::Notification,
                _ => crate::contact::ContactRole::ContactPerson,
            };
            contacts.push(Contact {
                contact_id,
                role,
                name: contact_row.name,
                affiliation: contact_row.affiliation,
                email: contact_row.email,
                phone: contact_row.phone,
                address: contact_row.address,
            });
        }

        // Read event instances
        let instance_rows = sqlx::query!(
            "SELECT begin, end FROM event_instances WHERE event_id = ? ORDER BY begin",
            event_id_str
        )
        .fetch_all(db())
        .await?;

        let instances = instance_rows
            .into_iter()
            .map(|row| {
                Ok(EventInstance {
                    begin: TimeString::try_from(row.begin)
                        .map_err(|e| anyhow::anyhow!("Invalid begin TimeString: {}", e))?,
                    end: TimeString::try_from(row.end)
                        .map_err(|e| anyhow::anyhow!("Invalid end TimeString: {}", e))?,
                })
            })
            .collect::<Result<Vec<_>, anyhow::Error>>()?
            .into_boxed_slice();

        // The column holds the `sqlx::Type` encoding, which is what
        // `save_to_database` writes, so it is decoded the same way rather than
        // through `FromStr` — `FromStr` is the inverse of `Display` and parses
        // the human-facing labels the add-event form submits, which are a
        // different vocabulary ("professional development", not
        // "ProfessionalDevelopment").
        let event_type = event_row.event_type;

        Ok(EventAll {
            event_id,
            created: TimeString::try_from(event_row.created)
                .map_err(|e| anyhow::anyhow!("Invalid created TimeString: {}", e))?,
            flagged: event_row.flagged,
            admin_notes: event_row.admin_notes,
            welcomed: event_row.welcomed,
            page_creator_id: event_row
                .page_creator_id
                .parse::<Ulid>()
                .map_err(|e| anyhow::anyhow!("Invalid page_creator_id ULID: {}", e))?,
            page_editor_ids,
            event_last_editor: None,
            event_last_edited: None,
            scistarter_last_editor: None,
            scistarter_last_edited: None,
            name: event_row.name,
            tags,
            description: event_row.description,
            origin: event_row.origin,
            contacts: contacts.into_boxed_slice(),
            presenter: event_row.presenter,
            url: event_row.url,
            image: event_row.image,
            image_credit: event_row.image_credit,
            how_to_join: event_row.how_to_join,
            part_of: event_row
                .part_of
                .map(|s| {
                    s.parse::<Ulid>()
                        .map_err(|e| anyhow::anyhow!("Invalid part_of ULID: {}", e))
                })
                .transpose()?,
            location_type: event_row.location_type,
            entirely_online: event_row.entirely_online,
            location_name: event_row.location_name,
            location_description: event_row.location_description,
            address: event_row.address,
            city: event_row.city,
            zip: event_row.zip,
            state: event_row.state,
            country: event_row.country,
            regions: event_row.regions.flatten(),
            point: event_row.point.flatten(),
            event_type,
            ticketing_begins: event_row
                .ticketing_begins
                .map(|s| TimeString::try_from(s))
                .transpose()
                .map_err(|e| anyhow::anyhow!("Invalid ticketing_begins TimeString: {}", e))?,
            outcomes_label: event_row.outcomes_label,
            outcomes_link: event_row.outcomes_link,
            circuit_id: event_row.circuit_id,
            instances,
            exchange: event_row.exchange,
            reviewed: event_row.reviewed,
            facebook_event: FacebookEvent::from_columns(
                event_row.facebook_event,
                event_row.facebook_event_url,
                event_row.facebook_event_requested,
            ),
        })
    }
}
