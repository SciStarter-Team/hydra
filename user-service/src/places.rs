use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::dashboard::{UserPlace, UserPlaceAdd, UserPlaceDelete},
    service::decode_structured_message,
    shared::db,
    ulid::AsUlid,
};
use ulid::Ulid;

pub async fn list_by_user(req: &Request) -> Result<Vec<UserPlace>, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;

    let rows = sqlx::query!(
        r#"SELECT
            "id",
            "name",
            ST_X("point") as "longitude!: f64",
            ST_Y("point") as "latitude!: f64",
            "created"
        FROM "user_places"
        WHERE "user_id" = ?
        ORDER BY "created" ASC"#,
        user_id
    )
    .fetch_all(db())
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(UserPlace {
                id: row.id.as_ulid()?,
                name: row.name,
                latitude: row.latitude,
                longitude: row.longitude,
                created: row.created,
            })
        })
        .collect()
}

pub async fn add(req: &Request) -> Result<UserPlace, Error> {
    let add: UserPlaceAdd = decode_structured_message(req.message.clone())?;
    let user_id_str = add.user_id.to_string();
    let id = Ulid::new();
    let id_str = id.to_string();
    let geojson = format!(
        r#"{{"type":"Point","coordinates":[{},{}]}}"#,
        add.longitude, add.latitude,
    );

    let row = sqlx::query!(
        r#"INSERT INTO "user_places" ("id", "user_id", "name", "point")
        VALUES (?, ?, ?, SetSRID(GeomFromGeoJSON(?), 4326))
        RETURNING "created""#,
        id_str,
        user_id_str,
        add.name,
        geojson,
    )
    .fetch_one(db())
    .await?;

    Ok(UserPlace {
        id,
        name: add.name,
        latitude: add.latitude,
        longitude: add.longitude,
        created: row.created,
    })
}

pub async fn delete(req: &Request) -> Result<(), Error> {
    let delete: UserPlaceDelete = decode_structured_message(req.message.clone())?;
    let id_str = delete.id.to_string();
    let user_id_str = delete.user_id.to_string();

    sqlx::query!(
        r#"DELETE FROM "user_places" WHERE "id" = ? AND "user_id" = ?"#,
        id_str,
        user_id_str,
    )
    .execute(db())
    .await?;

    Ok(())
}
