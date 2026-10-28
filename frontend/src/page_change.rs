//! Telling whether a write changed a project or an event, and how a change to
//! a page is logged.
//!
//! When a page was last updated is the activity log's to say (see
//! [`PageChange`]), so a write that changes nothing must log nothing, or the
//! page would look updated when it was not. Imports and the orrery sync write
//! a whole record whether or not anything in it is new, so they compare what
//! is held before and after the write with [`Stored`].

use std::collections::HashMap;

use common::{
    message::log::{Housekeeping, LastChangedRequest, LastChangedResponse, PageChange},
    service::{log_activity, request},
    timestring::TimeString,
};
use ulid::Ulid;

use crate::{
    component::detail_page::DetailPageKind,
    entity::{Entity, EntityKind},
    event::all::EventAll,
    peering::LoadStorePeered,
    project::all::ProjectAll,
};

/// What this instance holds of a project or event and its page, so that
/// whether a write changed any of it can be told by comparing what is held
/// before and after.
///
/// A copy written to every instance is compared on each of them, so the
/// comparison is also what keeps one change from being logged once per
/// instance: a peer that has already saved the copy it was sent finds
/// nothing changed.
#[derive(PartialEq)]
pub struct Stored<T> {
    /// The record in its canonical form, if there is one.
    record: Option<T>,
    /// The slug of its page, and whether the page is published.
    page: Option<(String, bool)>,
}

impl<T: PartialEq + serde::Serialize> Stored<T> {
    /// What a write is logged as having changed, in the place of the field
    /// an edit on the page names: what differs between `self`, held before
    /// the write, and `after`, followed by where the write came from.
    pub fn edit(&self, after: &Stored<T>, from: &str) -> String {
        let mut fields = match (&self.record, &after.record) {
            (Some(before), Some(after)) => changed_fields(before, after),
            _ => vec!["all".to_string()],
        };

        match (&self.page, &after.page) {
            (Some((before_slug, before_publish)), Some((after_slug, after_publish))) => {
                if before_slug != after_slug {
                    fields.push("slug".to_string());
                }
                if before_publish != after_publish {
                    fields.push("publish".to_string());
                }
            }
            (None, Some(_)) => fields.push("page".to_string()),
            _ => {}
        }

        format!("{} (from {from})", fields.join(", "))
    }

    /// Logs a write to `target` coming `from` somewhere as `update`, made by
    /// `user`, when it changed anything between `self`, held before the
    /// write, and `after`. A write that changed nothing logs nothing.
    pub async fn log_update(
        &self,
        after: &Stored<T>,
        update: PageChange,
        user: Option<Ulid>,
        target: Ulid,
        from: &str,
    ) -> Result<(), anyhow::Error> {
        if after == self {
            return Ok(());
        }

        log_activity(None, update, user, target, self.edit(after, from)).await
    }
}

impl Stored<ProjectAll> {
    pub async fn of_project(project_id: Ulid) -> Stored<ProjectAll> {
        Stored {
            record: ProjectAll::load(project_id)
                .await
                .ok()
                .map(ProjectAll::canonical),
            page: page(EntityKind::Project, project_id).await,
        }
    }
}

impl Stored<EventAll> {
    pub async fn of_event(event_id: Ulid) -> Stored<EventAll> {
        Stored {
            record: EventAll::load(event_id).await.ok().map(EventAll::canonical),
            page: page(EntityKind::Event, event_id).await,
        }
    }
}

/// The names of the fields that differ between two canonical forms. Tags
/// are a set held in no particular order, so their order is not a
/// difference.
fn changed_fields<T: serde::Serialize>(before: &T, after: &T) -> Vec<String> {
    use serde_json::Value;

    let (Ok(Value::Object(before)), Ok(Value::Object(after))) =
        (serde_json::to_value(before), serde_json::to_value(after))
    else {
        return vec!["all".to_string()];
    };

    let sorted = |value: &Value| {
        let mut items: Vec<String> = match value {
            Value::Array(items) => items.iter().map(Value::to_string).collect(),
            other => vec![other.to_string()],
        };
        items.sort_unstable();
        items
    };

    after
        .iter()
        .filter(|(field, value)| match before.get(*field) {
            Some(was) if field.as_str() == "tags" => sorted(was) != sorted(value),
            Some(was) => was != *value,
            None => true,
        })
        .map(|(field, _)| field.clone())
        .collect()
}

/// The slug of the page of `target`, and whether it is published.
async fn page(kind: EntityKind, target: Ulid) -> Option<(String, bool)> {
    Entity::page_of(kind, target)
        .await
        .ok()
        .flatten()
        .map(|page| (page.slug().to_string(), page.publish()))
}

/// When each of `targets` was last updated, for those updated since the
/// log began.
pub async fn last_changes(targets: Vec<Ulid>) -> Result<HashMap<Ulid, TimeString>, anyhow::Error> {
    let mut changes = HashMap::new();
    for batch in targets.chunks(LastChangedRequest::MAX_TARGETS) {
        let response: LastChangedResponse = request(
            "log.last_changed",
            LastChangedRequest {
                targets: batch.to_vec(),
            },
        )
        .await?;
        changes.extend(
            response
                .changes
                .into_iter()
                .map(|change| (change.target, change.when)),
        );
    }
    Ok(changes)
}

/// The action a staff change to an entity of `kind` is logged as. An
/// entity is the address of a page; creating or changing the address of a
/// content page is part of making that page, which is staff's to make, but
/// the address of a project's or an event's page is upkeep of it.
pub fn entity_change(kind: &EntityKind, change: PageChange) -> &'static str {
    match kind {
        EntityKind::Project => Housekeeping::Project.action(),
        EntityKind::Event => Housekeeping::Event.action(),
        _ => change.action(),
    }
}

/// The action an edit to a project or an event is logged as: an update
/// when its own people make it, housekeeping when staff do.
pub fn record_edit(kind: DetailPageKind, by_staff: bool) -> &'static str {
    match (kind, by_staff) {
        (DetailPageKind::Project, false) => PageChange::ProjectEdit.action(),
        (DetailPageKind::Event, false) => PageChange::EventEdit.action(),
        (DetailPageKind::Project, true) => Housekeeping::Project.action(),
        (DetailPageKind::Event, true) => Housekeeping::Event.action(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    #[derive(PartialEq, serde::Serialize)]
    struct Record {
        name: &'static str,
        tags: Vec<&'static str>,
    }

    fn stored(name: &'static str, tags: &[&'static str], page: (&str, bool)) -> Stored<Record> {
        Stored {
            record: Some(Record {
                name,
                tags: tags.to_vec(),
            }),
            page: Some((page.0.to_string(), page.1)),
        }
    }

    #[test]
    fn own_people_update_a_record_and_staff_keep_it() {
        check!(record_edit(DetailPageKind::Project, false) == "project-edit");
        check!(record_edit(DetailPageKind::Event, false) == "event-edit");
        check!(record_edit(DetailPageKind::Project, true) == "project-housekeeping");
        check!(record_edit(DetailPageKind::Event, true) == "event-housekeeping");
    }

    #[test]
    fn the_address_of_a_record_page_is_upkeep() {
        check!(
            entity_change(&EntityKind::Project, PageChange::EntityUpdate) == "project-housekeeping"
        );
        check!(entity_change(&EntityKind::Event, PageChange::EntityCreate) == "event-housekeeping");
        check!(
            entity_change(&EntityKind::PageBuilder, PageChange::EntityUpdate) == "entity-update"
        );
    }

    #[test]
    fn an_edit_names_what_changed_and_where_from() {
        let before = stored("Quoll Count", &["mammals", "night"], ("quoll-count", false));
        let after = stored(
            "Quoll Census",
            &["night", "mammals"],
            ("quoll-census", true),
        );

        check!(before.edit(&after, "the API") == "name, slug, publish (from the API)");
    }

    #[test]
    fn reordered_tags_are_no_change() {
        let before = stored("Quoll Count", &["mammals", "night"], ("quoll-count", true));
        let after = stored("Quoll Count", &["night", "mammals"], ("quoll-count", true));

        check!(before.edit(&after, "orrery") == " (from orrery)");
    }

    #[test]
    fn a_first_write_changes_all() {
        let before = Stored::<Record> {
            record: None,
            page: None,
        };
        let after = stored("Quoll Count", &[], ("quoll-count", false));

        check!(before.edit(&after, "the event scanner") == "all, page (from the event scanner)");
    }
}
