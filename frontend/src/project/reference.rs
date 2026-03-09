use anyhow::Error;
use common::shared::db;
use ulid::Ulid;

use crate::peering::LoadStorePeered;

#[derive(
    Debug,
    Copy,
    Clone,
    Default,
    sqlx::Type,
    serde::Serialize,
    serde::Deserialize,
    strum::AsRefStr,
    strum::EnumString,
    strum::Display,
    PartialEq,
)]
pub enum ReferenceType {
    #[default]
    Blog,
    #[strum(serialize = "Popular publication")]
    #[serde(rename = "Popular publication")]
    PopularPublication,
    #[strum(serialize = "Peer reviewed publication")]
    #[serde(rename = "Peer reviewed publication")]
    PeerReviewed,
    Reports,
    #[strum(serialize = "Other media")]
    #[serde(rename = "Other media")]
    OtherMedia,
    #[strum(serialize = "Other publications")]
    #[serde(rename = "Other publications")]
    OtherPublications,
    #[strum(serialize = "List of media coverage")]
    #[serde(rename = "List of media coverage")]
    MediaCoverageList,
    #[strum(serialize = "Publications list")]
    #[serde(rename = "Publications list")]
    PublicationsList,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectReference {
    pub reference_id: Ulid,
    pub entity_id: Ulid,
    pub label: String,
    pub reference_type: ReferenceType,
    pub url: String,
    pub citation: String,
    pub detailed_citation: String,
    pub sort_order: i32,
}

struct ProjectReferenceDB {
    reference_id: String,
    entity_id: String,
    label: String,
    reference_type: String,
    url: String,
    citation: String,
    detailed_citation: String,
    sort_order: i64,
}

impl TryFrom<ProjectReferenceDB> for ProjectReference {
    type Error = anyhow::Error;

    fn try_from(value: ProjectReferenceDB) -> Result<Self, Self::Error> {
        Ok(ProjectReference {
            reference_id: value.reference_id.parse()?,
            entity_id: value.entity_id.parse()?,
            label: value.label,
            reference_type: value.reference_type.parse().unwrap_or_default(),
            url: value.url,
            citation: value.citation,
            detailed_citation: value.detailed_citation,
            sort_order: value.sort_order as i32,
        })
    }
}

impl ProjectReference {
    pub async fn get_for_entity(entity_id: Ulid) -> Result<Vec<ProjectReference>, Error> {
        let entity_id_str = entity_id.to_string();
        let items = sqlx::query_as!(
            ProjectReferenceDB,
            r#"SELECT reference_id, entity_id, label, reference_type, url, citation, detailed_citation, sort_order
               FROM project_references
               WHERE entity_id = ?
               ORDER BY sort_order ASC"#,
            entity_id_str,
        )
        .fetch_all(db())
        .await?;

        items
            .into_iter()
            .map(|row| row.try_into())
            .collect::<Result<Vec<_>, _>>()
    }
}

impl LoadStorePeered for ProjectReference {
    type DatabaseKey = Ulid;

    const GROUP: &str = "project-reference";

    #[tracing::instrument]
    async fn read_from_database(reference_id: Self::DatabaseKey) -> Result<Self, Error> {
        let reference_id_str = reference_id.to_string();
        Ok(sqlx::query_as!(
            ProjectReferenceDB,
            r#"SELECT reference_id, entity_id, label, reference_type, url, citation, detailed_citation, sort_order
               FROM project_references
               WHERE reference_id = ?"#,
            reference_id_str,
        )
        .fetch_one(db())
        .await?
        .try_into()?)
    }

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        let reference_id_str = self.reference_id.to_string();
        let entity_id_str = self.entity_id.to_string();
        let reference_type_str = self.reference_type.to_string();
        let sort_order_i64 = self.sort_order as i64;

        sqlx::query!(
            r#"INSERT OR REPLACE INTO project_references (
                 reference_id, entity_id, label, reference_type, url, citation, detailed_citation, sort_order
               )
               VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
            reference_id_str,
            entity_id_str,
            self.label,
            reference_type_str,
            self.url,
            self.citation,
            self.detailed_citation,
            sort_order_i64,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    #[tracing::instrument]
    async fn delete_from_database(reference_id: Self::DatabaseKey) -> Result<(), Error> {
        let reference_id_str = reference_id.to_string();
        sqlx::query!(
            r#"DELETE FROM project_references WHERE reference_id = ?"#,
            reference_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }
}
