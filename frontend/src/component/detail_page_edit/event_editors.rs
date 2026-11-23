//! The event page's own editors, gathered for staff to change an event with
//! from the admin. Each part changes there just as an organizer changes it on
//! the event page, through the same requests and under the same rules, so
//! the admin edits everything the event page does without a copy of either.

use std::collections::HashMap;

use anyhow::Error;
use ulid::Ulid;

use super::{
    DetailPageKind, EventAdmissionDisplay, EventAttendanceDisplay, EventLocationDisplay,
    EventPartOfDisplay, EventRegistrationDisplay, FieldDisplay, InstancesDisplay, ManagersDisplay,
    current_field_display, load_manager_ids, units::EventUnit,
};

/// Every editor of an event, open to staff.
pub struct EventEditors {
    pub fields: FieldEditors,
    pub location: EventLocationDisplay,
    pub instances: InstancesDisplay,
    pub registration: EventRegistrationDisplay,
    pub admission: EventAdmissionDisplay,
    pub attendance: EventAttendanceDisplay,
    pub part_of: EventPartOfDisplay,
    pub managers: ManagersDisplay,
}

impl EventEditors {
    /// The event page's fields edited one at a time, apart from those edited
    /// as part of where the event is held, which `location` holds.
    pub const FIELDS: &[&'static str] = &[
        "image",
        "presenter",
        "name",
        "description",
        "url",
        "how_to_join",
        "formats",
        "programs",
        "venues",
        "topics",
        "tags",
    ];

    /// The editors of the event `event_id`, as it now stands.
    pub async fn load(event_id: Ulid) -> Result<Self, Error> {
        let id = event_id.to_string();
        let mut fields = Vec::with_capacity(Self::FIELDS.len());
        for &field in Self::FIELDS {
            fields.push((
                field,
                current_field_display(&id, field, DetailPageKind::Event, true, true).await?,
            ));
        }
        let managers = load_manager_ids(DetailPageKind::Event, event_id).await?;
        Ok(EventEditors {
            fields: fields.into_iter().collect(),
            location: EventLocationDisplay::load(&id, true, true).await?,
            instances: InstancesDisplay::load(&id, true).await?,
            registration: EventRegistrationDisplay::load(&id, true, true).await?,
            admission: EventAdmissionDisplay::load(&id, true, true).await?,
            attendance: EventAttendanceDisplay::load(&id, true, true).await?,
            part_of: EventPartOfDisplay::load(&id, true, true).await?,
            managers: ManagersDisplay::of(id, &managers, None).await,
        })
    }
}

/// The editors of the event's fields edited one at a time.
pub struct FieldEditors(HashMap<&'static str, FieldDisplay>);

impl FieldEditors {
    /// The editor of `field`, one of `EventEditors::FIELDS`.
    pub fn field(&self, field: &str) -> FieldDisplay {
        self.0
            .get(field)
            .cloned()
            .unwrap_or_else(|| panic!("{field} isn't one of EventEditors::FIELDS"))
    }
}

impl FromIterator<(&'static str, FieldDisplay)> for FieldEditors {
    fn from_iter<I: IntoIterator<Item = (&'static str, FieldDisplay)>>(editors: I) -> Self {
        FieldEditors(editors.into_iter().collect())
    }
}
