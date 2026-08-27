//! PageBuilderComponent - Reusable template components with slots

use anyhow::{Error, anyhow};
use common::{shared::db, timestring::TimeString};
use ulid::Ulid;

use crate::builder::render::CompiledTemplate;
use crate::peering::LoadStorePeered;

/// A reusable page component with template source containing slots
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageBuilderComponent {
    pub component_id: Ulid,
    pub name: String,
    pub description: String,
    pub source: String,
    pub category: String,
    pub created: TimeString,
    pub updated: TimeString,
}

impl PageBuilderComponent {
    /// Check that `source` is fit to be a component.
    ///
    /// Two things are refused. Source that does not compile, which is the
    /// check every template gets. And source that translates text as it
    /// renders, which only a component has to worry about: placing one
    /// renders it and splices the HTML into the page
    /// ([`crate::pagebuilder::source::compose_component_into_slot`]), so a
    /// `{$ tr "..." $}` here would be translated into the language of
    /// whoever placed the component and frozen there.
    ///
    /// That failure is silent -- the page looks right to the person who made
    /// it, and only a reader in another language ever sees the difference --
    /// so it is refused at the one moment somebody can still act on it.
    /// Written `{$! tr "..." $}` the directive survives placement, at any
    /// depth of nesting, and is translated for each reader.
    pub fn check_source(source: &str) -> Result<(), Error> {
        let compiled = CompiledTemplate::compile(source)?;

        let mut baked: Vec<&str> = Vec::new();

        for phrase in compiled.translated_phrases() {
            if !baked.contains(&phrase) {
                baked.push(phrase);
            }
        }

        if baked.is_empty() {
            return Ok(());
        }

        let listed: String = baked
            .iter()
            .map(|phrase| format!("\n  \u{2022} {phrase}"))
            .collect();

        Err(anyhow!(
            "A component's source is rendered every time the component is \
             placed, so these phrases would be translated once and frozen in \
             whichever language the person placing it was reading. Write \
             {{$! tr \"...\" $}} instead -- the ! means the phrase is \
             translated when a reader is served, however the component is \
             placed or nested:{listed}"
        ))
    }

    /// Create a new component with the given name and source
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        let now = TimeString::now();
        Self {
            component_id: Ulid::new(),
            name: name.into(),
            description: String::new(),
            source: source.into(),
            category: "general".to_string(),
            created: now.clone(),
            updated: now,
        }
    }

    /// Create a new component with a specific ID
    pub fn new_with_id(
        component_id: Ulid,
        name: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        let now = TimeString::now();
        Self {
            component_id,
            name: name.into(),
            description: String::new(),
            source: source.into(),
            category: "general".to_string(),
            created: now.clone(),
            updated: now,
        }
    }

    /// Set the component description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the component category
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    /// Update the source and refresh the updated timestamp
    pub fn set_source(&mut self, source: impl Into<String>) {
        self.source = source.into();
        self.updated = TimeString::now();
    }

    /// List all components, optionally filtered by category
    pub async fn list(category: Option<&str>) -> Result<Vec<Self>, Error> {
        struct Row {
            component_id: String,
            name: String,
            description: String,
            source: String,
            category: String,
            created: String,
            updated: String,
        }

        let rows = if let Some(cat) = category {
            sqlx::query_as!(
                Row,
                r#"SELECT component_id, name, description, source, category, created, updated
                   FROM pagebuilder_components
                   WHERE category = ?
                   ORDER BY name"#,
                cat
            )
            .fetch_all(db())
            .await?
        } else {
            sqlx::query_as!(
                Row,
                r#"SELECT component_id, name, description, source, category, created, updated
                   FROM pagebuilder_components
                   ORDER BY name"#
            )
            .fetch_all(db())
            .await?
        };

        rows.into_iter()
            .map(|row| {
                Ok(Self {
                    component_id: row.component_id.parse()?,
                    name: row.name,
                    description: row.description,
                    source: row.source,
                    category: row.category,
                    created: TimeString::try_from(row.created)?,
                    updated: TimeString::try_from(row.updated)?,
                })
            })
            .collect()
    }

    /// Get distinct categories
    pub async fn categories() -> Result<Vec<String>, Error> {
        struct Row {
            category: String,
        }

        let rows = sqlx::query_as!(
            Row,
            r#"SELECT DISTINCT category FROM pagebuilder_components ORDER BY category"#
        )
        .fetch_all(db())
        .await?;

        Ok(rows.into_iter().map(|r| r.category).collect())
    }
}

impl LoadStorePeered for PageBuilderComponent {
    type DatabaseKey = Ulid;

    const GROUP: &str = "pagebuilder_component";

    async fn save_to_database(&self) -> Result<(), Error> {
        let component_id_str = self.component_id.to_string();
        let created_str = self.created.to_string();
        let updated_str = self.updated.to_string();

        sqlx::query!(
            r#"INSERT OR REPLACE INTO pagebuilder_components
               (component_id, name, description, source, category, created, updated)
               VALUES (?, ?, ?, ?, ?, ?, ?)"#,
            component_id_str,
            self.name,
            self.description,
            self.source,
            self.category,
            created_str,
            updated_str,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, Error> {
        struct Row {
            component_id: String,
            name: String,
            description: String,
            source: String,
            category: String,
            created: String,
            updated: String,
        }

        let key_str = key.to_string();

        let row = sqlx::query_as!(
            Row,
            r#"SELECT component_id, name, description, source, category, created, updated
               FROM pagebuilder_components
               WHERE component_id = ?"#,
            key_str
        )
        .fetch_one(db())
        .await?;

        Ok(Self {
            component_id: row.component_id.parse()?,
            name: row.name,
            description: row.description,
            source: row.source,
            category: row.category,
            created: TimeString::try_from(row.created)?,
            updated: TimeString::try_from(row.updated)?,
        })
    }

    async fn delete_from_database(key: Self::DatabaseKey) -> Result<(), Error> {
        let key_str = key.to_string();

        sqlx::query!(
            r#"DELETE FROM pagebuilder_components WHERE component_id = ?"#,
            key_str
        )
        .execute(db())
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    /// The form a component is meant to be written in: the directive
    /// survives placement -- every placement, at any depth -- so each reader
    /// is translated for separately.
    #[test]
    fn a_deferred_phrase_is_what_a_component_wants() {
        check!(PageBuilderComponent::check_source(r#"<p>{$! tr "Hello" $}</p>"#).is_ok());
    }

    /// The mistake this exists to catch, and the message has to name the
    /// phrase, because a component may hold a great many of them.
    #[test]
    fn an_unescaped_phrase_is_refused_by_name() {
        let refusal = PageBuilderComponent::check_source(r#"<p>{$ tr "Hello" $}</p>"#)
            .expect_err("an unescaped phrase should be refused");

        let message = refusal.to_string();

        check!(message.contains("Hello"));
        // And names the remedy, which is the notation rather than a count.
        check!(message.contains("{$! tr"));
    }

    /// A phrase can sit anywhere a body can, so the walk has to go inside
    /// every construct rather than looking only at the top level.
    #[test]
    fn a_phrase_is_found_wherever_a_body_can_hold_one() {
        for source in [
            r#"{$ if flag $}{$ tr "Hidden" $}{$ /if $}"#,
            r#"{$ if flag $}x{$ else $}{$ tr "Hidden" $}{$ /if $}"#,
            r#"{$ each item in items $}{$ tr "Hidden" $}{$ /each $}"#,
            r#"{$ times 3 $}{$ tr "Hidden" $}{$ /times $}"#,
            r#"{$ slot main $}{$ tr "Hidden" $}{$ /slot $}"#,
        ] {
            let refused = PageBuilderComponent::check_source(source).is_err();

            check!(refused, "should have been refused: {source}");
        }
    }

    /// One phrase written twice is one thing to fix, so it is said once.
    #[test]
    fn a_repeated_phrase_is_listed_once() {
        let refusal =
            PageBuilderComponent::check_source(r#"{$ tr "Twice" $}{$ tr "Twice" $}"#).unwrap_err();

        check!(refusal.to_string().matches("Twice").count() == 1);
    }

    #[test]
    fn source_that_does_not_compile_is_still_refused() {
        check!(PageBuilderComponent::check_source("{$ if unclosed $}").is_err());
    }

    /// A component with no translation in it is not this check's business.
    #[test]
    fn an_ordinary_component_passes() {
        check!(
            PageBuilderComponent::check_source(
                r#"{$ param title default="Hi" $}<h1>{$= title $}</h1>{$ slot body $}{$ /slot $}"#
            )
            .is_ok()
        );
    }
}
