use ulid::Ulid;

#[derive(sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum EntityKind {
    Redirect,
    RawHTML,
    Wysiwyg,
    PageBuilder,
    Form,
    Project,
    Event,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Entity {
    slug: String,
    kind: EntityKind,
    target: Ulid,
}
