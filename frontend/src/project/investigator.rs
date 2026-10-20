use anyhow::Error;
use common::shared::db;
use ulid::Ulid;

use crate::peering::LoadStorePeered;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectInvestigator {
    pub investigator_id: Ulid,
    pub entity_id: Ulid,
    pub name: String,
    pub affiliation: String,
    pub sort_order: i32,
}

struct ProjectInvestigatorDB {
    investigator_id: String,
    entity_id: String,
    name: String,
    affiliation: String,
    sort_order: i64,
}

impl TryFrom<ProjectInvestigatorDB> for ProjectInvestigator {
    type Error = anyhow::Error;

    fn try_from(value: ProjectInvestigatorDB) -> Result<Self, Self::Error> {
        Ok(ProjectInvestigator {
            investigator_id: value.investigator_id.parse()?,
            entity_id: value.entity_id.parse()?,
            name: value.name,
            affiliation: value.affiliation,
            sort_order: value.sort_order as i32,
        })
    }
}

impl ProjectInvestigator {
    pub async fn get_for_entity(entity_id: Ulid) -> Result<Vec<ProjectInvestigator>, Error> {
        let entity_id_str = entity_id.to_string();
        let items = sqlx::query_as!(
            ProjectInvestigatorDB,
            r#"SELECT investigator_id, entity_id, name, affiliation, sort_order
               FROM project_investigators
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

impl LoadStorePeered for ProjectInvestigator {
    type DatabaseKey = Ulid;

    const GROUP: &str = "project-investigator";

    #[tracing::instrument]
    async fn read_from_database(investigator_id: Self::DatabaseKey) -> Result<Self, Error> {
        let investigator_id_str = investigator_id.to_string();
        Ok(sqlx::query_as!(
            ProjectInvestigatorDB,
            r#"SELECT investigator_id, entity_id, name, affiliation, sort_order
               FROM project_investigators
               WHERE investigator_id = ?"#,
            investigator_id_str,
        )
        .fetch_one(db())
        .await?
        .try_into()?)
    }

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        let investigator_id_str = self.investigator_id.to_string();
        let entity_id_str = self.entity_id.to_string();
        let sort_order_i64 = self.sort_order as i64;

        sqlx::query!(
            r#"INSERT INTO project_investigators (
                 investigator_id, entity_id, name, affiliation, sort_order
               )
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT (investigator_id) DO UPDATE SET
                 entity_id = excluded.entity_id,
                 name = excluded.name,
                 affiliation = excluded.affiliation,
                 sort_order = excluded.sort_order"#,
            investigator_id_str,
            entity_id_str,
            self.name,
            self.affiliation,
            sort_order_i64,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    #[tracing::instrument]
    async fn delete_from_database(investigator_id: Self::DatabaseKey) -> Result<(), Error> {
        let investigator_id_str = investigator_id.to_string();
        sqlx::query!(
            r#"DELETE FROM project_investigators WHERE investigator_id = ?"#,
            investigator_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }
}
