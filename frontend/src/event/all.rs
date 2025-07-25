use std::collections::HashSet;

use common::{
    insensitivestring::InsensitiveString, shared::db, timestring::TimeString,
    transaction::Immediate,
};
use ulid::Ulid;

use crate::{contact::Contact, peering::LoadStorePeered};

use super::{EventInstance, EventType};

#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EventAll {
    pub event_id: Ulid,
    pub created: TimeString,
    pub publish: bool,
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
}

impl LoadStorePeered for EventAll {
    type DatabaseKey = Ulid;

    const GROUP: &str = "event";
    const SPECIFIC: &str = "all";

    async fn save_to_database(&self) -> Result<(), anyhow::Error> {
        let event_id = self.event_id.to_string();
        let part_of = self.part_of.map(|u| u.to_string());
        let page_creator_id = self.page_creator_id.to_string();

        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

        sqlx::query!(
            r#"insert or replace into "events" (
              "event_id",
              "created",
              "publish",
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
              ?30,
              case
                when ?31 is null then
                  null
                when substr(?31, 1, 1) == '{' and substr(?31, -1, 1) == '}' then
                  GeomFromGeoJSON(?31)
                else
                  GeomFromEWKT(?31)
              end,
              case
                when ?32 is null then
                  null
                when substr(?32, 1, 1) == '{' and substr(?32, -1, 1) == '}' then
                  GeomFromGeoJSON(?32)
                else
                  GeomFromEWKT(?32)
              end
            )"#,
            event_id,
            self.created,
            self.publish,
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
            common::service::send(
                "log.event.user-edit",
                (last_editor, self.event_last_edited.to_owned(), "all"),
            )
            .await?;
        }

        // "scistarter_last_editor",
        // "scistarter_last_edited",
        if let Some(last_editor) = self.scistarter_last_editor {
            common::service::send(
                "log.event.staff-edit",
                (last_editor, self.scistarter_last_edited.to_owned(), "all"),
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

        Ok(())
    }

    async fn read_from_database(_key: Self::DatabaseKey) -> Result<Self, anyhow::Error> {
        unimplemented!("Implementation deferred until needed")
    }
}
