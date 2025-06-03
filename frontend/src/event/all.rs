use common::{shared::db, timestring::TimeString};
use ulid::Ulid;

use crate::{contact::Contact, peering::LoadStorePeered};

use super::{EventInstance, EventType};

#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EventAll {
    pub event_id: Ulid,
    pub flagged: bool,
    pub redirect: Option<String>,
    pub admin_notes: String,
    pub authoritative: bool,
    pub welcomed: bool,
    pub description: String,
    pub guid: String,
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
    pub ticketing_begins: TimeString,
    pub outcomes_label: String,
    pub outcomes_link: String,
    pub circuit_id: String,
    pub instances: Box<[EventInstance]>,
}

impl LoadStorePeered for EventAll {
    type DatabaseKey = Ulid;

    const GROUP: &str = "event";
    const SPECIFIC: &str = "all";

    async fn save_to_database(&self) -> Result<(), anyhow::Error> {
        let db = db();

        let event_id = self.event_id.to_string();
        let part_of = self.part_of.map(|u| u.to_string());

        sqlx::query!(
            r#"insert or replace into "events" (
              "event_id",
              "flagged",
              "redirect",
              "admin_notes",
              "authoritative",
              "welcomed",
              "description",
              "guid",
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
              "point"
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
                  GeomFromGeoJSON(?30)
                else
                  GeomFromEWKT(?30)
              end,
              case
                when ?31 is null then
                  null
                when substr(?31, 1, 1) == '{' and substr(?31, -1, 1) == '}' then
                  GeomFromGeoJSON(?31)
                else
                  GeomFromEWKT(?31)
              end
            )"#,
            event_id,
            self.flagged,
            self.redirect,
            self.admin_notes,
            self.authoritative,
            self.welcomed,
            self.description,
            self.guid,
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
        )
        .execute(db)
        .await?;

        //pub contacts: Box<[Contact]>,
        let tx = db.begin().await?;
        sqlx::query!(r#"delete from event_contacts where event_id = ?"#, event_id)
            .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        // pub instances: Box<[EventInstance]>,
        let tx = db.begin().await?;
        sqlx::query!(
            r#"delete from event_instances where event_id = ?"#,
            event_id
        )
        .execute(db)
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
            .execute(db)
            .await?;
        }
        tx.commit().await?;

        Ok(())
    }

    async fn read_from_database(_key: Self::DatabaseKey) -> Result<Self, anyhow::Error> {
        unimplemented!("Implementation deferred until needed")
    }
}
