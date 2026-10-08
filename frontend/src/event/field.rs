use crate::{
    contact::{Contact, ContactRole},
    event::EventStatus,
    peering::LoadStorePeered,
};
use anyhow::Error;
use common::timestring::TimeString;
use ulid::Ulid;

// Note: The `tags_text` database column is NOT included in EventFieldType
// as it's a computed field for FTS search optimization, not a real event field.
// Tags are managed separately via the event_tags table and dedicated handlers.

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct EventField {
    pub event_id: Ulid,
    pub field: EventFieldValue,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, strum::EnumDiscriminants)]
#[strum_discriminants(name(EventFieldType), derive(serde::Serialize, serde::Deserialize))]
pub enum EventFieldValue {
    EventId(Ulid),
    Status(EventStatus),
    Created(TimeString),
    Flagged(bool),
    AdminNotes(String),
    Welcomed(bool),
    PageCreatorId(Ulid),
    Name(String),
    Description(String),
    Origin(String),
    Presenter(String),
    Url(String),
    Image(String),
    ImageCredit(String),
    HowToJoin(String),
    PartOf(Option<Ulid>),
    LocationType(String),
    EntirelyOnline(bool),
    LocationName(String),
    LocationDescription(String),
    Address(String),
    City(String),
    Zip(String),
    State(String),
    Country(String),
    EventType(crate::event::EventType),
    TicketingBegins(Option<TimeString>),
    OutcomesLabel(Option<String>),
    OutcomesLink(Option<String>),
    CircuitId(Option<String>),
    Point(Option<String>),
    Regions(Option<String>),
    Contacts(Box<[Contact]>),
    PageEditorIds(Box<[Ulid]>),
    Reviewed(bool),
}

impl EventField {
    /// Create a new EventField with the given event_id and field value
    pub fn new(event_id: Ulid, field: EventFieldValue) -> Self {
        Self { event_id, field }
    }

    #[tracing::instrument]
    pub fn field_type(&self) -> EventFieldType {
        match &self.field {
            EventFieldValue::EventId(_) => EventFieldType::EventId,
            EventFieldValue::Status(_) => EventFieldType::Status,
            EventFieldValue::Created(_) => EventFieldType::Created,
            EventFieldValue::Flagged(_) => EventFieldType::Flagged,
            EventFieldValue::AdminNotes(_) => EventFieldType::AdminNotes,
            EventFieldValue::Welcomed(_) => EventFieldType::Welcomed,
            EventFieldValue::PageCreatorId(_) => EventFieldType::PageCreatorId,
            EventFieldValue::Name(_) => EventFieldType::Name,
            EventFieldValue::Description(_) => EventFieldType::Description,
            EventFieldValue::Origin(_) => EventFieldType::Origin,
            EventFieldValue::Presenter(_) => EventFieldType::Presenter,
            EventFieldValue::Url(_) => EventFieldType::Url,
            EventFieldValue::Image(_) => EventFieldType::Image,
            EventFieldValue::ImageCredit(_) => EventFieldType::ImageCredit,
            EventFieldValue::HowToJoin(_) => EventFieldType::HowToJoin,
            EventFieldValue::PartOf(_) => EventFieldType::PartOf,
            EventFieldValue::LocationType(_) => EventFieldType::LocationType,
            EventFieldValue::EntirelyOnline(_) => EventFieldType::EntirelyOnline,
            EventFieldValue::LocationName(_) => EventFieldType::LocationName,
            EventFieldValue::LocationDescription(_) => EventFieldType::LocationDescription,
            EventFieldValue::Address(_) => EventFieldType::Address,
            EventFieldValue::City(_) => EventFieldType::City,
            EventFieldValue::Zip(_) => EventFieldType::Zip,
            EventFieldValue::State(_) => EventFieldType::State,
            EventFieldValue::Country(_) => EventFieldType::Country,
            EventFieldValue::EventType(_) => EventFieldType::EventType,
            EventFieldValue::TicketingBegins(_) => EventFieldType::TicketingBegins,
            EventFieldValue::OutcomesLabel(_) => EventFieldType::OutcomesLabel,
            EventFieldValue::OutcomesLink(_) => EventFieldType::OutcomesLink,
            EventFieldValue::CircuitId(_) => EventFieldType::CircuitId,
            EventFieldValue::Point(_) => EventFieldType::Point,
            EventFieldValue::Regions(_) => EventFieldType::Regions,
            EventFieldValue::Contacts(_) => EventFieldType::Contacts,
            EventFieldValue::PageEditorIds(_) => EventFieldType::PageEditorIds,
            EventFieldValue::Reviewed(_) => EventFieldType::Reviewed,
        }
    }

    /// Replace the event's contact list. Mirrors how `EventAll` persists
    /// `event_contacts` (delete-then-reinsert, atomically).
    async fn save_contacts(&self, contacts: &[Contact]) -> Result<(), Error> {
        use common::shared::db;
        use common::transaction::Immediate;

        let event_id = self.event_id.to_string();
        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

        sqlx::query!(r#"delete from event_contacts where event_id = ?"#, event_id)
            .execute(&mut tx)
            .await?;

        for contact in contacts {
            let contact_id = contact.contact_id.to_string();
            sqlx::query!(
                r#"insert into event_contacts (
                  "contact_id", "event_id", "role", "name",
                  "affiliation", "email", "phone", "address"
                ) values (?, ?, ?, ?, ?, ?, ?, ?)"#,
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

        tx.commit().await?;
        Ok(())
    }

    async fn read_contacts(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;

        let event_id_str = event_id.to_string();
        let rows = sqlx::query!(
            r#"SELECT contact_id, role, name, affiliation, email, phone, address
               FROM event_contacts WHERE event_id = ? ORDER BY role"#,
            event_id_str
        )
        .fetch_all(db())
        .await?;

        let mut contacts = Vec::with_capacity(rows.len());
        for row in rows {
            let contact_id = row
                .contact_id
                .parse::<Ulid>()
                .map_err(|e| anyhow::anyhow!("Invalid contact ULID: {e}"))?;
            let role = match row.role.as_str() {
                "Manager" => ContactRole::Manager,
                "ContactPerson" => ContactRole::ContactPerson,
                "Technical" => ContactRole::Technical,
                "Notification" => ContactRole::Notification,
                _ => ContactRole::ContactPerson,
            };
            contacts.push(Contact {
                contact_id,
                role,
                name: row.name,
                affiliation: row.affiliation,
                email: row.email,
                phone: row.phone,
                address: row.address,
            });
        }

        Ok(EventField::new(
            event_id,
            EventFieldValue::Contacts(contacts.into_boxed_slice()),
        ))
    }

    /// Replace the event's non-staff page editors (managers).
    async fn save_page_editor_ids(&self, ids: &[Ulid]) -> Result<(), Error> {
        use common::shared::db;
        use common::transaction::Immediate;

        let event_id = self.event_id.to_string();
        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

        sqlx::query!(
            r#"delete from event_page_editors where event_id = ?"#,
            event_id
        )
        .execute(&mut tx)
        .await?;

        for user_id in ids {
            let user_id = user_id.to_string();
            sqlx::query!(
                r#"insert into event_page_editors ("event_id", "user_id") values (?, ?)"#,
                event_id,
                user_id,
            )
            .execute(&mut tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn read_page_editor_ids(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;

        let event_id_str = event_id.to_string();
        let rows = sqlx::query!(
            r#"SELECT user_id FROM event_page_editors WHERE event_id = ? ORDER BY user_id"#,
            event_id_str
        )
        .fetch_all(db())
        .await?;

        let mut ids = Vec::with_capacity(rows.len());
        for row in rows {
            ids.push(
                row.user_id
                    .parse::<Ulid>()
                    .map_err(|e| anyhow::anyhow!("Invalid editor ULID: {e}"))?,
            );
        }

        Ok(EventField::new(
            event_id,
            EventFieldValue::PageEditorIds(ids.into_boxed_slice()),
        ))
    }

    // Helper functions for database operations
    async fn save_status(&self, status: &EventStatus) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        let status_str = match status {
            EventStatus::Active => "Active",
            EventStatus::Cancelled => "Cancelled",
            EventStatus::Expired => "Expired",
        };

        sqlx::query!(
            "UPDATE events SET status = ? WHERE event_id = ?",
            status_str,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_created(&self, created: &TimeString) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        let created_str = created.to_string();
        sqlx::query!(
            "UPDATE events SET created = ? WHERE event_id = ?",
            created_str,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_reviewed(&self, reviewed: &bool) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET reviewed = ? WHERE event_id = ?",
            reviewed,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_flagged(&self, flagged: &bool) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET flagged = ? WHERE event_id = ?",
            flagged,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_admin_notes(&self, notes: &str) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET admin_notes = ? WHERE event_id = ?",
            notes,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_welcomed(&self, welcomed: &bool) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET welcomed = ? WHERE event_id = ?",
            welcomed,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_page_creator_id(&self, creator_id: &Ulid) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        let creator_id_str = creator_id.to_string();
        sqlx::query!(
            "UPDATE events SET page_creator_id = ? WHERE event_id = ?",
            creator_id_str,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_name(&self, name: &str) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET name = ? WHERE event_id = ?",
            name,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_description(&self, description: &str) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET description = ? WHERE event_id = ?",
            description,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_origin(&self, origin: &str) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET origin = ? WHERE event_id = ?",
            origin,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_presenter(&self, presenter: &str) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET presenter = ? WHERE event_id = ?",
            presenter,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_url(&self, url: &str) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET url = ? WHERE event_id = ?",
            url,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_image(&self, image: &str) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET image = ? WHERE event_id = ?",
            image,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_image_credit(&self, image_credit: &str) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET image_credit = ? WHERE event_id = ?",
            image_credit,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_how_to_join(&self, how_to_join: &str) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET how_to_join = ? WHERE event_id = ?",
            how_to_join,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_part_of(&self, part_of: &Option<Ulid>) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        let part_of_str = part_of.map(|u| u.to_string());
        sqlx::query!(
            "UPDATE events SET part_of = ? WHERE event_id = ?",
            part_of_str,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_location_type(&self, location_type: &str) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET location_type = ? WHERE event_id = ?",
            location_type,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn save_entirely_online(&self, entirely_online: &bool) -> Result<(), Error> {
        use common::shared::db;

        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET entirely_online = ? WHERE event_id = ?",
            entirely_online,
            event_id_str
        )
        .execute(db())
        .await?;

        Ok(())
    }

    // Location and remaining fields
    async fn save_location_name(&self, location_name: &str) -> Result<(), Error> {
        use common::shared::db;
        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET location_name = ? WHERE event_id = ?",
            location_name,
            event_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_location_description(&self, location_description: &str) -> Result<(), Error> {
        use common::shared::db;
        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET location_description = ? WHERE event_id = ?",
            location_description,
            event_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_address(&self, address: &str) -> Result<(), Error> {
        use common::shared::db;
        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET address = ? WHERE event_id = ?",
            address,
            event_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_city(&self, city: &str) -> Result<(), Error> {
        use common::shared::db;
        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET city = ? WHERE event_id = ?",
            city,
            event_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_zip(&self, zip: &str) -> Result<(), Error> {
        use common::shared::db;
        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET zip = ? WHERE event_id = ?",
            zip,
            event_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_state(&self, state: &str) -> Result<(), Error> {
        use common::shared::db;
        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET state = ? WHERE event_id = ?",
            state,
            event_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_country(&self, country: &str) -> Result<(), Error> {
        use common::shared::db;
        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET country = ? WHERE event_id = ?",
            country,
            event_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_event_type(&self, event_type: &crate::event::EventType) -> Result<(), Error> {
        use common::shared::db;
        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET event_type = ? WHERE event_id = ?",
            event_type,
            event_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_ticketing_begins(
        &self,
        ticketing_begins: &Option<TimeString>,
    ) -> Result<(), Error> {
        use common::shared::db;
        let event_id_str = self.event_id.to_string();
        let ticketing_str = ticketing_begins.as_ref().map(|ts| ts.to_string());
        sqlx::query!(
            "UPDATE events SET ticketing_begins = ? WHERE event_id = ?",
            ticketing_str,
            event_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_outcomes_label(&self, outcomes_label: &Option<String>) -> Result<(), Error> {
        use common::shared::db;
        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET outcomes_label = ? WHERE event_id = ?",
            outcomes_label,
            event_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_outcomes_link(&self, outcomes_link: &Option<String>) -> Result<(), Error> {
        use common::shared::db;
        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET outcomes_link = ? WHERE event_id = ?",
            outcomes_link,
            event_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_circuit_id(&self, circuit_id: &Option<String>) -> Result<(), Error> {
        use common::shared::db;
        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            "UPDATE events SET circuit_id = ? WHERE event_id = ?",
            circuit_id,
            event_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_point(&self, point: &Option<String>) -> Result<(), Error> {
        use common::shared::db;
        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            r#"UPDATE events SET point = case
            when ?1 is null then null
            when substr(?1, 1, 1) == '{' and substr(?1, -1, 1) == '}' then SetSRID(GeomFromGeoJSON(?1), 4326)
            else GeomFromEWKT(?1)
        end WHERE event_id = ?2"#,
            point,
            event_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    async fn save_regions(&self, regions: &Option<String>) -> Result<(), Error> {
        use common::shared::db;
        let event_id_str = self.event_id.to_string();
        sqlx::query!(
            r#"UPDATE events SET regions = case
            when ?1 is null then null
            when substr(?1, 1, 1) == '{' and substr(?1, -1, 1) == '}' then SetSRID(GeomFromGeoJSON(?1), 4326)
            else GeomFromEWKT(?1)
        end WHERE event_id = ?2"#,
            regions,
            event_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }

    // Helper functions for reading from database
    async fn read_status(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;

        let event_id_str = event_id.to_string();
        let status_str =
            sqlx::query_scalar!("SELECT status FROM events WHERE event_id = ?", event_id_str)
                .fetch_one(db())
                .await?;

        let status = match status_str.as_str() {
            "Active" => EventStatus::Active,
            "Cancelled" => EventStatus::Cancelled,
            "Expired" => EventStatus::Expired,
            _ => EventStatus::Active, // Default fallback
        };

        Ok(EventField::new(event_id, EventFieldValue::Status(status)))
    }

    // Read helpers for all other field types
    async fn read_created(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let created_str = sqlx::query_scalar!(
            "SELECT created FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        let created = TimeString::try_from(created_str)?;
        Ok(EventField::new(event_id, EventFieldValue::Created(created)))
    }

    async fn read_reviewed(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let reviewed = sqlx::query_scalar!(
            r#"SELECT reviewed as "reviewed: bool" FROM events WHERE event_id = ?"#,
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::Reviewed(reviewed),
        ))
    }

    async fn read_flagged(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let flagged = sqlx::query_scalar!(
            "SELECT flagged FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(event_id, EventFieldValue::Flagged(flagged)))
    }

    async fn read_admin_notes(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let admin_notes = sqlx::query_scalar!(
            "SELECT admin_notes FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::AdminNotes(admin_notes),
        ))
    }

    async fn read_welcomed(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let welcomed = sqlx::query_scalar!(
            "SELECT welcomed FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::Welcomed(welcomed),
        ))
    }

    async fn read_page_creator_id(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let creator_id_str = sqlx::query_scalar!(
            "SELECT page_creator_id FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        let creator_id = creator_id_str.parse::<Ulid>()?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::PageCreatorId(creator_id),
        ))
    }

    async fn read_name(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let name = sqlx::query_scalar!("SELECT name FROM events WHERE event_id = ?", event_id_str)
            .fetch_one(db())
            .await?;
        Ok(EventField::new(event_id, EventFieldValue::Name(name)))
    }

    async fn read_description(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let description = sqlx::query_scalar!(
            "SELECT description FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::Description(description),
        ))
    }

    async fn read_origin(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let origin =
            sqlx::query_scalar!("SELECT origin FROM events WHERE event_id = ?", event_id_str)
                .fetch_one(db())
                .await?;
        Ok(EventField::new(event_id, EventFieldValue::Origin(origin)))
    }

    async fn read_presenter(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let presenter = sqlx::query_scalar!(
            "SELECT presenter FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::Presenter(presenter),
        ))
    }

    async fn read_url(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let url = sqlx::query_scalar!("SELECT url FROM events WHERE event_id = ?", event_id_str)
            .fetch_one(db())
            .await?;
        Ok(EventField::new(event_id, EventFieldValue::Url(url)))
    }

    async fn read_image(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let image =
            sqlx::query_scalar!("SELECT image FROM events WHERE event_id = ?", event_id_str)
                .fetch_one(db())
                .await?;
        Ok(EventField::new(event_id, EventFieldValue::Image(image)))
    }

    async fn read_image_credit(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let image_credit = sqlx::query_scalar!(
            "SELECT image_credit FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::ImageCredit(image_credit),
        ))
    }

    async fn read_how_to_join(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let how_to_join = sqlx::query_scalar!(
            "SELECT how_to_join FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::HowToJoin(how_to_join),
        ))
    }

    async fn read_part_of(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let part_of_str: Option<String> = sqlx::query_scalar!(
            "SELECT part_of FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        let part_of = part_of_str.map(|s| s.parse::<Ulid>()).transpose()?;
        Ok(EventField::new(event_id, EventFieldValue::PartOf(part_of)))
    }

    // Add remaining read helpers compactly
    async fn read_location_type(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let location_type = sqlx::query_scalar!(
            "SELECT location_type FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::LocationType(location_type),
        ))
    }

    async fn read_entirely_online(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let entirely_online = sqlx::query_scalar!(
            "SELECT entirely_online FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::EntirelyOnline(entirely_online),
        ))
    }

    async fn read_location_name(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let location_name = sqlx::query_scalar!(
            "SELECT location_name FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::LocationName(location_name),
        ))
    }

    async fn read_location_description(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let location_description = sqlx::query_scalar!(
            "SELECT location_description FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::LocationDescription(location_description),
        ))
    }

    async fn read_address(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let address = sqlx::query_scalar!(
            "SELECT address FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(event_id, EventFieldValue::Address(address)))
    }

    async fn read_city(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let city = sqlx::query_scalar!("SELECT city FROM events WHERE event_id = ?", event_id_str)
            .fetch_one(db())
            .await?;
        Ok(EventField::new(event_id, EventFieldValue::City(city)))
    }

    async fn read_zip(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let zip = sqlx::query_scalar!("SELECT zip FROM events WHERE event_id = ?", event_id_str)
            .fetch_one(db())
            .await?;
        Ok(EventField::new(event_id, EventFieldValue::Zip(zip)))
    }

    async fn read_state(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let state =
            sqlx::query_scalar!("SELECT state FROM events WHERE event_id = ?", event_id_str)
                .fetch_one(db())
                .await?;
        Ok(EventField::new(event_id, EventFieldValue::State(state)))
    }

    async fn read_country(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let country = sqlx::query_scalar!(
            "SELECT country FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(event_id, EventFieldValue::Country(country)))
    }

    async fn read_event_type(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let event_type = sqlx::query_scalar!(
            r#"SELECT event_type as "event_type: crate::event::EventType" FROM events WHERE event_id = ?"#,
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::EventType(event_type),
        ))
    }

    async fn read_ticketing_begins(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let ticketing_str: Option<String> = sqlx::query_scalar!(
            "SELECT ticketing_begins FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        let ticketing_begins = ticketing_str.and_then(|s| TimeString::try_from(s).ok());
        Ok(EventField::new(
            event_id,
            EventFieldValue::TicketingBegins(ticketing_begins),
        ))
    }

    async fn read_outcomes_label(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let outcomes_label = sqlx::query_scalar!(
            "SELECT outcomes_label FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::OutcomesLabel(outcomes_label),
        ))
    }

    async fn read_outcomes_link(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let outcomes_link = sqlx::query_scalar!(
            "SELECT outcomes_link FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::OutcomesLink(outcomes_link),
        ))
    }

    async fn read_circuit_id(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let circuit_id = sqlx::query_scalar!(
            "SELECT circuit_id FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(
            event_id,
            EventFieldValue::CircuitId(circuit_id),
        ))
    }

    async fn read_point(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let point: Option<String> = sqlx::query_scalar!(
            "SELECT AsGeoJSON(point) as 'point: String' FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(event_id, EventFieldValue::Point(point)))
    }

    async fn read_regions(event_id: Ulid) -> Result<Self, Error> {
        use common::shared::db;
        let event_id_str = event_id.to_string();
        let regions: Option<String> = sqlx::query_scalar!(
            "SELECT AsGeoJSON(regions) as 'regions: String' FROM events WHERE event_id = ?",
            event_id_str
        )
        .fetch_one(db())
        .await?;
        Ok(EventField::new(event_id, EventFieldValue::Regions(regions)))
    }
}

impl LoadStorePeered for EventField {
    type DatabaseKey = (Ulid, EventFieldType);

    const GROUP: &str = "event";
    const SPECIFIC: &str = "field";

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        match &self.field {
            EventFieldValue::Status(status) => self.save_status(status).await,
            EventFieldValue::Created(created) => self.save_created(created).await,
            EventFieldValue::Flagged(flagged) => self.save_flagged(flagged).await,
            EventFieldValue::AdminNotes(notes) => self.save_admin_notes(notes).await,
            EventFieldValue::Welcomed(welcomed) => self.save_welcomed(welcomed).await,
            EventFieldValue::PageCreatorId(creator_id) => {
                self.save_page_creator_id(creator_id).await
            }
            EventFieldValue::Name(name) => self.save_name(name).await,
            EventFieldValue::Description(description) => self.save_description(description).await,
            EventFieldValue::Origin(origin) => self.save_origin(origin).await,
            EventFieldValue::Presenter(presenter) => self.save_presenter(presenter).await,
            EventFieldValue::Url(url) => self.save_url(url).await,
            EventFieldValue::Image(image) => self.save_image(image).await,
            EventFieldValue::ImageCredit(image_credit) => {
                self.save_image_credit(image_credit).await
            }
            EventFieldValue::HowToJoin(how_to_join) => self.save_how_to_join(how_to_join).await,
            EventFieldValue::PartOf(part_of) => self.save_part_of(part_of).await,
            EventFieldValue::LocationType(location_type) => {
                self.save_location_type(location_type).await
            }
            EventFieldValue::EntirelyOnline(entirely_online) => {
                self.save_entirely_online(entirely_online).await
            }
            EventFieldValue::LocationName(location_name) => {
                self.save_location_name(location_name).await
            }
            EventFieldValue::LocationDescription(location_description) => {
                self.save_location_description(location_description).await
            }
            EventFieldValue::Address(address) => {
                let result = self.save_address(address).await;
                if result.is_ok() {
                    if let Err(e) =
                        crate::geocode::geocode_and_update_event_point(self.event_id).await
                    {
                        eprintln!("Geocoding after address save failed: {e}");
                    }
                }
                result
            }
            EventFieldValue::City(city) => {
                let result = self.save_city(city).await;
                if result.is_ok() {
                    if let Err(e) =
                        crate::geocode::geocode_and_update_event_point(self.event_id).await
                    {
                        eprintln!("Geocoding after city save failed: {e}");
                    }
                }
                result
            }
            EventFieldValue::Zip(zip) => self.save_zip(zip).await,
            EventFieldValue::State(state) => {
                let result = self.save_state(state).await;
                if result.is_ok() {
                    if let Err(e) =
                        crate::geocode::geocode_and_update_event_point(self.event_id).await
                    {
                        eprintln!("Geocoding after state save failed: {e}");
                    }
                }
                result
            }
            EventFieldValue::Country(country) => {
                let result = self.save_country(country).await;
                if result.is_ok() {
                    if let Err(e) =
                        crate::geocode::geocode_and_update_event_point(self.event_id).await
                    {
                        eprintln!("Geocoding after country save failed: {e}");
                    }
                }
                result
            }
            EventFieldValue::EventType(event_type) => self.save_event_type(event_type).await,
            EventFieldValue::TicketingBegins(ticketing_begins) => {
                self.save_ticketing_begins(ticketing_begins).await
            }
            EventFieldValue::OutcomesLabel(outcomes_label) => {
                self.save_outcomes_label(outcomes_label).await
            }
            EventFieldValue::OutcomesLink(outcomes_link) => {
                self.save_outcomes_link(outcomes_link).await
            }
            EventFieldValue::CircuitId(circuit_id) => self.save_circuit_id(circuit_id).await,
            EventFieldValue::Point(point) => self.save_point(point).await,
            EventFieldValue::Regions(regions) => self.save_regions(regions).await,
            EventFieldValue::Contacts(contacts) => self.save_contacts(contacts).await,
            EventFieldValue::PageEditorIds(ids) => self.save_page_editor_ids(ids).await,
            EventFieldValue::Reviewed(reviewed) => self.save_reviewed(reviewed).await,
            EventFieldValue::EventId(_) => {
                // EventId is immutable and should not be updated
                Err(anyhow::anyhow!("EventId cannot be updated"))
            }
        }
    }

    #[tracing::instrument]
    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, Error> {
        let (event_id, field_type) = key;

        match field_type {
            EventFieldType::EventId => Ok(EventField::new(
                event_id,
                EventFieldValue::EventId(event_id),
            )),
            EventFieldType::Status => Self::read_status(event_id).await,
            EventFieldType::Created => Self::read_created(event_id).await,
            EventFieldType::Flagged => Self::read_flagged(event_id).await,
            EventFieldType::AdminNotes => Self::read_admin_notes(event_id).await,
            EventFieldType::Welcomed => Self::read_welcomed(event_id).await,
            EventFieldType::PageCreatorId => Self::read_page_creator_id(event_id).await,
            EventFieldType::Name => Self::read_name(event_id).await,
            EventFieldType::Description => Self::read_description(event_id).await,
            EventFieldType::Origin => Self::read_origin(event_id).await,
            EventFieldType::Presenter => Self::read_presenter(event_id).await,
            EventFieldType::Url => Self::read_url(event_id).await,
            EventFieldType::Image => Self::read_image(event_id).await,
            EventFieldType::ImageCredit => Self::read_image_credit(event_id).await,
            EventFieldType::HowToJoin => Self::read_how_to_join(event_id).await,
            EventFieldType::PartOf => Self::read_part_of(event_id).await,
            EventFieldType::LocationType => Self::read_location_type(event_id).await,
            EventFieldType::EntirelyOnline => Self::read_entirely_online(event_id).await,
            EventFieldType::LocationName => Self::read_location_name(event_id).await,
            EventFieldType::LocationDescription => Self::read_location_description(event_id).await,
            EventFieldType::Address => Self::read_address(event_id).await,
            EventFieldType::City => Self::read_city(event_id).await,
            EventFieldType::Zip => Self::read_zip(event_id).await,
            EventFieldType::State => Self::read_state(event_id).await,
            EventFieldType::Country => Self::read_country(event_id).await,
            EventFieldType::EventType => Self::read_event_type(event_id).await,
            EventFieldType::TicketingBegins => Self::read_ticketing_begins(event_id).await,
            EventFieldType::OutcomesLabel => Self::read_outcomes_label(event_id).await,
            EventFieldType::OutcomesLink => Self::read_outcomes_link(event_id).await,
            EventFieldType::CircuitId => Self::read_circuit_id(event_id).await,
            EventFieldType::Point => Self::read_point(event_id).await,
            EventFieldType::Regions => Self::read_regions(event_id).await,
            EventFieldType::Contacts => Self::read_contacts(event_id).await,
            EventFieldType::PageEditorIds => Self::read_page_editor_ids(event_id).await,
            EventFieldType::Reviewed => Self::read_reviewed(event_id).await,
        }
    }
}
