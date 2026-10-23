//! A training page's editable copy: the text its code ships with -- its
//! built-in copy -- with whatever staff have changed in the Engine Room laid
//! over it.
//!
//! Only the changes are stored. A field nobody has touched is not in the
//! database at all, so the page shows whatever the code currently says, and a
//! correction to the built-in copy reaches the page with the next deploy. A
//! record that held every field would pin all of them at the moment of its
//! first save: the trainings ran that way until October 2026, and their rows
//! filled up with copies of old built-in text that later code changes could
//! no longer reach.
//!
//! Each training's copy is a typed struct implementing [`TrainingCopy`]; the
//! changes to one record of it are a [`TrainingOverrides`], one row of
//! `training_content`, peered across frontends when stored.

use std::collections::BTreeMap;

use anyhow::Error;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};

use crate::peering::LoadStorePeered;
use common::shared::db;

/// A training's editable copy.
#[allow(async_fn_in_trait)]
pub trait TrainingCopy: Serialize + DeserializeOwned {
    /// Which of the training's records: `()` for a training with one page,
    /// the language for one with a page per language.
    type Record: Copy;

    /// The `training_content` row holding a record's changes.
    fn row(record: Self::Record) -> &'static str;

    /// The copy a record ships with.
    fn built_in(record: Self::Record) -> Self;

    /// A record's copy as staff have left it: its built-in copy with their
    /// changes laid over it. If the changes cannot be read, the page shows
    /// the built-in copy rather than failing, and the failure is logged.
    async fn current(record: Self::Record) -> Self {
        let built_in = Self::built_in(record);
        let row = Self::row(record);

        let overrides = match TrainingOverrides::load(row.to_string()).await {
            Ok(overrides) if overrides.fields.is_empty() => return built_in,
            Ok(overrides) => overrides,
            Err(err) => {
                tracing::error!("reading training copy changes for {row}: {err}");
                return built_in;
            }
        };

        let current = copy_fields(&built_in)
            .map(|fields| overrides.apply(fields))
            .and_then(|fields| Ok(serde_json::from_value(Value::Object(fields))?));

        match current {
            Ok(current) => current,
            Err(err) => {
                tracing::error!("applying training copy changes for {row}: {err}");
                built_in
            }
        }
    }
}

/// A copy's fields, by name, as JSON.
pub fn copy_fields<C: Serialize>(copy: &C) -> Result<Map<String, Value>, Error> {
    match serde_json::to_value(copy)? {
        Value::Object(fields) => Ok(fields),
        other => anyhow::bail!("training copy serialized as {other}, not as an object"),
    }
}

/// The changes staff have made to one record of a training's copy: just the
/// fields whose text differs from the built-in copy, by name.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrainingOverrides {
    /// The `training_content` row these belong to.
    pub row: String,

    /// The changed fields, as JSON values of the copy's own field types.
    pub fields: BTreeMap<String, Value>,
}

impl TrainingOverrides {
    /// The changes that turn `built_in` into `edited`: every field of
    /// `edited` whose value differs from the built-in one. A field edited to
    /// read exactly as the built-in copy does is not a change, and is left to
    /// follow the code.
    pub fn between(
        row: &str,
        built_in: &Map<String, Value>,
        edited: &Map<String, Value>,
    ) -> TrainingOverrides {
        let fields = edited
            .iter()
            .filter(|(name, value)| built_in.get(*name) != Some(*value))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();

        TrainingOverrides {
            row: row.to_string(),
            fields,
        }
    }

    /// `built_in` with these changes laid over it.
    ///
    /// A change to a field the copy no longer has, or whose stored value is
    /// no longer the kind of value the copy holds there, is a leftover of an
    /// older version of the copy; the built-in text stands in its place.
    /// Line endings are normalized as they are for a new edit, because text
    /// saved from a browser's textarea arrives with CRLF line endings that
    /// mean nothing and would otherwise make an unchanged field look edited.
    pub fn apply(&self, mut built_in: Map<String, Value>) -> Map<String, Value> {
        for (name, value) in &self.fields {
            if let Some(slot) = built_in.get_mut(name) {
                if same_kind(slot, value) {
                    *slot = normalize_line_endings(value.clone());
                }
            }
        }

        built_in
    }
}

/// Whether two values are the same kind of JSON -- both strings, both lists,
/// and so on -- so that one can stand in for the other in a copy.
fn same_kind(a: &Value, b: &Value) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// Rewrites CRLF line endings, anywhere in `value`, as LF.
pub fn normalize_line_endings(value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(text.replace("\r\n", "\n")),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(normalize_line_endings).collect())
        }
        other => other,
    }
}

impl LoadStorePeered for TrainingOverrides {
    type DatabaseKey = String;

    const GROUP: &str = "training_content";

    /// Writes the row, or removes it once nothing is changed any more.
    async fn save_to_database(&self) -> Result<(), Error> {
        if self.fields.is_empty() {
            sqlx::query!(
                r#"DELETE FROM "training_content" WHERE "workflow" = ?"#,
                self.row,
            )
            .execute(db())
            .await?;
        } else {
            let json = serde_json::to_string(&self.fields)?;
            sqlx::query!(
                r#"INSERT OR REPLACE INTO "training_content" ("workflow", "content") VALUES (?, ?)"#,
                self.row,
                json,
            )
            .execute(db())
            .await?;
        }

        Ok(())
    }

    /// A row that does not exist is a record nobody has changed.
    async fn read_from_database(row: String) -> Result<Self, Error> {
        let stored = sqlx::query_scalar!(
            r#"SELECT "content" FROM "training_content" WHERE "workflow" = ?"#,
            row,
        )
        .fetch_optional(db())
        .await?;

        let fields = match stored {
            Some(json) => serde_json::from_str(&json)?,
            None => BTreeMap::new(),
        };

        Ok(TrainingOverrides { row, fields })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(fields) => fields,
            _ => panic!("not an object"),
        }
    }

    /// Only the fields that read differently from the built-in copy are
    /// recorded, so the rest go on following the code.
    #[test]
    fn only_fields_that_differ_from_the_built_in_copy_are_changes() {
        let built_in = fields(json!({ "title": "Built in", "intro": "Same", "skills": ["a"] }));
        let edited = fields(json!({ "title": "Edited", "intro": "Same", "skills": ["a"] }));

        let overrides = TrainingOverrides::between("row", &built_in, &edited);

        assert_eq!(
            overrides.fields,
            BTreeMap::from([("title".to_string(), json!("Edited"))])
        );
    }

    /// Changes lay over the built-in copy; everything else reads as the
    /// code currently has it, including a field added after the change was
    /// made.
    #[test]
    fn changes_lay_over_the_current_built_in_copy() {
        let overrides = TrainingOverrides {
            row: "row".to_string(),
            fields: BTreeMap::from([("title".to_string(), json!("Edited"))]),
        };

        let current = overrides.apply(fields(json!({ "title": "Built in", "added": "New" })));

        assert_eq!(
            current,
            fields(json!({ "title": "Edited", "added": "New" }))
        );
    }

    /// A change to a field the copy has since dropped, or that now holds a
    /// different kind of value, is a leftover and does not reach the page.
    #[test]
    fn leftover_changes_are_ignored() {
        let overrides = TrainingOverrides {
            row: "row".to_string(),
            fields: BTreeMap::from([
                ("removed".to_string(), json!("Gone")),
                ("skills".to_string(), json!("was a string once")),
            ]),
        };

        let current = overrides.apply(fields(json!({ "skills": ["a", "b"] })));

        assert_eq!(current, fields(json!({ "skills": ["a", "b"] })));
    }

    /// Text that came back from a textarea differs from the built-in copy
    /// only in its line endings; it reads as the built-in text, so the next
    /// save drops it.
    #[test]
    fn crlf_line_endings_do_not_make_a_change() {
        let built_in = fields(json!({ "html": "<p>a</p>\n<p>b</p>", "skills": ["x\ny"] }));
        let overrides = TrainingOverrides {
            row: "row".to_string(),
            fields: BTreeMap::from([
                ("html".to_string(), json!("<p>a</p>\r\n<p>b</p>")),
                ("skills".to_string(), json!(["x\r\ny"])),
            ]),
        };

        let current = overrides.apply(built_in.clone());

        assert_eq!(current, built_in);
        assert!(
            TrainingOverrides::between("row", &built_in, &current)
                .fields
                .is_empty()
        );
    }

    /// A record whose every field was saved -- as every record was before
    /// only changes were kept -- reads back exactly as it was saved.
    #[test]
    fn a_record_holding_every_field_reads_back_whole() {
        let saved = json!({ "title": "Saved", "skills": ["s"] });
        let overrides = TrainingOverrides {
            row: "row".to_string(),
            fields: serde_json::from_value(saved.clone()).unwrap(),
        };

        let current = overrides.apply(fields(json!({ "title": "Built in", "skills": [] })));

        assert_eq!(Value::Object(current), saved);
    }

    /// Peers receive the changes as flexbuffers, so the JSON values inside
    /// have to survive that trip.
    #[test]
    fn overrides_survive_the_trip_to_a_peer() {
        let overrides = TrainingOverrides {
            row: "row".to_string(),
            fields: BTreeMap::from([
                ("title".to_string(), json!("Edited")),
                ("skills".to_string(), json!(["a", "b"])),
            ]),
        };

        let bytes = flexbuffers::to_vec(&overrides).unwrap();
        let received: TrainingOverrides = flexbuffers::from_slice(&bytes).unwrap();

        assert_eq!(received, overrides);
    }
}
