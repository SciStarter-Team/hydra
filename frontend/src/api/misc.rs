// Miscellaneous v1 API endpoints ported from orrery/orrery/api/views/misc.py

use anyhow::Error;
use common::message::user::{
    AmbassadorPassedLocationsRequest, AmbassadorPassedLocationsResponse, IdentifierType, Login,
    SnmProfile, TrainingStatesRequest, TrainingStatesResponse, UserMatchRequest, UserMatchResponse,
    WorkflowProgressRequest, WorkflowProgressResponse, WorkflowType,
};
use common::service::request;
use poem::{IntoResponse, Response, handler, http::StatusCode, web::Json};
use serde::Deserialize;

use crate::geocode::geocode_address_to_point;

/// POST /api/library-network-signup
///
/// Body: `{"ulid": "<user ulid>"}`. Advances that user's Libraries-as-Hubs
/// workflow by recording the supplemental signup. Always returns an empty 200;
/// an unknown or missing user is a silent no-op (matches orrery).
#[handler]
pub async fn library_network_signup(
    Json(body): Json<serde_json::Value>,
) -> Result<Response, Error> {
    let ulid = body
        .get("ulid")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if ulid.is_empty() {
        return Ok(StatusCode::OK.into());
    }

    // Confirm the ULID identifies a real user before advancing their workflow.
    let user_match: UserMatchResponse = request(
        "user.match",
        UserMatchRequest {
            identifier: ulid,
            identifier_type: IdentifierType::Ulid,
        },
    )
    .await?;

    if let Some(user_id) = user_match.user_id {
        let _: WorkflowProgressResponse = request(
            "user.workflow.progress",
            WorkflowProgressRequest {
                user_id,
                workflow: WorkflowType::LibrariesHubs,
                step: "supplemental".to_string(),
            },
        )
        .await?;
    }

    Ok(StatusCode::OK.into())
}

/// GET /api/training-state.json
///
/// Exports each user's current Foundations training state (opaque id + state),
/// excluding users still in the initial state. No authentication (matches orrery).
#[handler]
pub async fn training_state() -> Result<Response, Error> {
    let response: TrainingStatesResponse =
        request("user.workflow.training_states", TrainingStatesRequest {}).await?;

    let states: Vec<serde_json::Value> = response
        .states
        .into_iter()
        .map(|s| serde_json::json!({ "person": s.person, "state": s.state }))
        .collect();

    Ok(Json(serde_json::json!({ "states": states })).into_response())
}

/// GET /api/ambassador_map.geojson
///
/// Returns a GeoJSON FeatureCollection of distinct city/state points for
/// ambassadors who passed review. Locations are geocoded on demand (results are
/// cached by the geocode service). No authentication (matches orrery).
#[handler]
pub async fn ambassador_map() -> Result<Response, Error> {
    let response: AmbassadorPassedLocationsResponse = request(
        "user.ambassador.passed_locations",
        AmbassadorPassedLocationsRequest {},
    )
    .await?;

    let mut features = Vec::new();
    for location in response.locations {
        // geocode_address_to_point returns a GeoJSON Point string when resolved.
        let Some(point_json) = geocode_address_to_point(&location.city, &location.state, "").await
        else {
            continue;
        };
        let Ok(geometry) = serde_json::from_str::<serde_json::Value>(&point_json) else {
            continue;
        };
        features.push(serde_json::json!({
            "type": "Feature",
            "geometry": geometry,
            "properties": {
                "city": location.city,
                "state": location.state,
            }
        }));
    }

    Ok(Json(serde_json::json!({
        "type": "FeatureCollection",
        "features": features
    }))
    .into_response())
}

/// The NaCl `Box` envelope exchanged with Science Near Me. Each field is a raw
/// byte sequence sent as a JSON array of integers (matching orrery's PyNaCl
/// `[x for x in bytes(...)]` encoding).
#[derive(Debug, Deserialize)]
struct SnmSealed {
    source: Vec<u8>,
    ciphertext: Vec<u8>,
    nonce: Vec<u8>,
}

#[derive(Debug, serde::Serialize)]
struct SnmEnvelope {
    source: Vec<u8>,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct SnmCredentials {
    email: String,
    password: String,
}

/// Decode a base64-encoded 32-byte key from the environment.
fn snm_key_bytes(var: &str) -> Option<[u8; 32]> {
    use base64::Engine;
    let encoded = std::env::var(var).ok()?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    decoded.try_into().ok()
}

/// POST /api/login-for-snm
///
/// The Science Near Me login bridge. SNM sends a NaCl `Box`-encrypted
/// `{source, ciphertext, nonce}` envelope containing `{email, password}`; we
/// verify the credentials and return the user's profile, encrypted back to SNM
/// in the same envelope shape. Ported from orrery's `snm_login`.
#[handler]
pub async fn snm_login(Json(sealed): Json<SnmSealed>) -> Result<Response, Error> {
    use crypto_box::{
        PublicKey, SalsaBox, SecretKey,
        aead::{Aead, AeadCore, OsRng},
    };

    // Keys: our secret + SNM's public, each a raw 32-byte key, base64 in env.
    let (Some(sci_secret_bytes), Some(snm_public_bytes)) =
        (snm_key_bytes("SNM_SCI_SECRET"), snm_key_bytes("SNM_PUBLIC"))
    else {
        return Err(anyhow::anyhow!("Science Near Me bridge is not configured"));
    };

    let sci_secret = SecretKey::from(sci_secret_bytes);
    let snm_public = PublicKey::from(snm_public_bytes);
    let sci_public = sci_secret.public_key();
    let crypto = SalsaBox::new(&snm_public, &sci_secret);

    // The envelope's source must be SNM's public key.
    if sealed.source.as_slice() != snm_public.as_bytes().as_slice() {
        return Ok(StatusCode::FORBIDDEN.into());
    }

    // Decrypt the incoming credentials (nonce is XSalsa20's 24 bytes).
    if sealed.nonce.len() != 24 {
        return Ok(StatusCode::BAD_REQUEST.into());
    }
    let nonce = crypto_box::Nonce::from_slice(&sealed.nonce);
    let Ok(plaintext) = crypto.decrypt(nonce, sealed.ciphertext.as_slice()) else {
        return Ok(StatusCode::FORBIDDEN.into());
    };
    let Ok(creds) = serde_json::from_slice::<SnmCredentials>(&plaintext) else {
        return Ok(StatusCode::BAD_REQUEST.into());
    };

    // Verify the credentials via user-service.
    let profile: Option<SnmProfile> = request(
        "user.auth.snm_login",
        Login {
            email: creds.email,
            password: creds.password,
        },
    )
    .await?;

    let Some(profile) = profile else {
        // Authentication failed.
        return Ok(StatusCode::NOT_FOUND.into());
    };

    // Build and encrypt the response payload.
    let payload = serde_json::json!({
        "username": profile.username,
        "first_name": profile.first_name,
        "last_name": profile.last_name,
        "emails": profile.emails,
        "zip_code": profile.zip_code,
        "phone": serde_json::Value::Null,
        "newsletter": profile.newsletter,
    });
    let payload_bytes = serde_json::to_vec(&payload)?;

    let out_nonce = SalsaBox::generate_nonce(&mut OsRng);
    let out_ciphertext = crypto
        .encrypt(&out_nonce, payload_bytes.as_slice())
        .map_err(|_| anyhow::anyhow!("Failed to encrypt Science Near Me response"))?;

    let envelope = SnmEnvelope {
        source: sci_public.as_bytes().to_vec(),
        nonce: out_nonce.as_slice().to_vec(),
        ciphertext: out_ciphertext,
    };

    Ok(Json(envelope).into_response())
}
