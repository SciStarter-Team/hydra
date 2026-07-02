use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::dashboard::{AmbassadorAppStatus, AmbassadorApplication, AmbassadorApplicationUpdate},
    message::user::{AmbassadorLocation, AmbassadorPassedLocationsResponse},
    service::decode_structured_message,
    shared::db,
    ulid::AsUlid,
};

/// Distinct city/state locations of ambassadors who passed application review
/// (accepted or champion). Backs /api/ambassador_map.geojson; the frontend
/// geocodes each location for the map. "passed" maps to Accepted/Champion.
pub async fn passed_locations(_req: &Request) -> Result<AmbassadorPassedLocationsResponse, Error> {
    let accepted = AmbassadorAppStatus::Accepted as i64;
    let champion = AmbassadorAppStatus::Champion as i64;

    let rows = sqlx::query!(
        r#"SELECT DISTINCT city, state FROM ambassador_applications
           WHERE app_status IN (?, ?) AND city != ''
           ORDER BY city, state"#,
        accepted,
        champion,
    )
    .fetch_all(db())
    .await?;

    let locations = rows
        .into_iter()
        .map(|r| AmbassadorLocation {
            city: r.city,
            state: r.state,
        })
        .collect();

    Ok(AmbassadorPassedLocationsResponse { locations })
}

pub async fn get(req: &Request) -> Result<Option<AmbassadorApplication>, Error> {
    let user_id: String = flexbuffers::from_slice(&req.message.payload)?;
    let user_ulid = user_id.as_ulid()?;

    let row = sqlx::query!(
        r#"SELECT
            "bio",
            "city",
            "state",
            "contact_email",
            "youtube_video_url",
            "app_status",
            "submitted",
            "updated_at"
        FROM "ambassador_applications"
        WHERE "user_id" = ?"#,
        user_id
    )
    .fetch_optional(db())
    .await?;

    Ok(row.map(|row| AmbassadorApplication {
        user_id: user_ulid,
        bio: row.bio,
        city: row.city,
        state: row.state,
        contact_email: row.contact_email,
        youtube_video_url: row.youtube_video_url,
        app_status: AmbassadorAppStatus::from_i64(row.app_status),
        submitted: row.submitted,
        updated_at: row.updated_at,
    }))
}

pub async fn update(req: &Request) -> Result<(), Error> {
    let update: AmbassadorApplicationUpdate = decode_structured_message(req.message.clone())?;
    let user_id_str = update.user_id.to_string();

    sqlx::query!(
        r#"INSERT INTO "ambassador_applications"
            ("user_id", "bio", "city", "state", "contact_email", "youtube_video_url", "updated_at")
        VALUES (?, ?, ?, ?, ?, ?, current_timestamp)
        ON CONFLICT ("user_id") DO UPDATE SET
            "bio" = excluded."bio",
            "city" = excluded."city",
            "state" = excluded."state",
            "contact_email" = excluded."contact_email",
            "youtube_video_url" = excluded."youtube_video_url",
            "updated_at" = current_timestamp"#,
        user_id_str,
        update.bio,
        update.city,
        update.state,
        update.contact_email,
        update.youtube_video_url,
    )
    .execute(db())
    .await?;

    Ok(())
}
