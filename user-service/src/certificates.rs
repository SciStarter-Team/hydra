use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{
        CertificateCheckRequest, CertificateCheckResponse, CertificateCreateRequest,
        CertificateCreateResponse, CertificateGetRequest, CertificateGetResponse, CertificateInfo,
        UserCertificatesRequest, UserCertificatesResponse,
    },
    service::{TriggerEvent, trigger_email},
    shared::db,
};
use ulid::Ulid;

pub async fn create(req: &Request) -> Result<CertificateCreateResponse, Error> {
    let input: CertificateCreateRequest = flexbuffers::from_slice(&req.message.payload)?;
    create_certificate(input.user_id, input.project, input.mode).await
}

/// Issue a certificate of `mode` to `user_id` (deduped on `(user, project,
/// mode)`), firing the certificate email on first issue. Callable both from the
/// `user.certificates.create` handler and directly by in-process flows (e.g.
/// Champion training completion in [`crate::workflow`]).
pub async fn create_certificate(
    user_id: Ulid,
    project: String,
    mode: String,
) -> Result<CertificateCreateResponse, Error> {
    let user_id_str = user_id.to_string();

    // Check if certificate already exists
    let existing = sqlx::query_scalar!(
        "SELECT certificate_id FROM certificates WHERE user_id = ? AND project = ? AND mode = ?",
        user_id_str,
        project,
        mode,
    )
    .fetch_optional(db())
    .await?;

    if let Some(existing_id) = existing {
        return Ok(CertificateCreateResponse {
            certificate_id: existing_id.parse()?,
            already_existed: true,
        });
    }

    let certificate_id = Ulid::new();
    let certificate_id_str = certificate_id.to_string();
    let now = jiff::Timestamp::now().to_string();

    sqlx::query!(
        "INSERT INTO certificates (certificate_id, created_at, user_id, project, mode) VALUES (?, ?, ?, ?, ?)",
        certificate_id_str,
        now,
        user_id_str,
        project,
        mode,
    )
    .execute(db())
    .await?;

    trigger_email(
        TriggerEvent::CertificateEarned,
        user_id,
        // The mail links the certificate, so it needs the address rather than
        // the id -- orrery interpolates the same `{certificate_url}`.
        Some(&[
            (
                "certificate_url".to_string(),
                format!(
                    "https://{}/-certificate/{certificate_id_str}",
                    env!("DOMAIN")
                ),
            ),
            ("certificate_id".to_string(), certificate_id_str),
            ("project".to_string(), project),
            ("mode".to_string(), mode),
        ]),
    )
    .await
    .ok();

    Ok(CertificateCreateResponse {
        certificate_id,
        already_existed: false,
    })
}

pub async fn check(req: &Request) -> Result<CertificateCheckResponse, Error> {
    let input: CertificateCheckRequest = flexbuffers::from_slice(&req.message.payload)?;

    let user_id_str = input.user_id.to_string();

    let existing = sqlx::query_scalar!(
        "SELECT certificate_id FROM certificates WHERE user_id = ? AND project = ? AND mode = ?",
        user_id_str,
        input.project,
        input.mode,
    )
    .fetch_optional(db())
    .await?;

    Ok(CertificateCheckResponse {
        exists: existing.is_some(),
        certificate_id: existing.and_then(|id| id.parse().ok()),
    })
}

pub async fn list(req: &Request) -> Result<UserCertificatesResponse, Error> {
    let input: UserCertificatesRequest = flexbuffers::from_slice(&req.message.payload)?;

    let user_id_str = input.user_id.to_string();

    struct CertRow {
        certificate_id: String,
        created_at: String,
        user_id: String,
        project: String,
        mode: String,
    }

    let rows = sqlx::query_as!(
        CertRow,
        "SELECT certificate_id, created_at, user_id, project, mode FROM certificates WHERE user_id = ? ORDER BY created_at DESC",
        user_id_str,
    )
    .fetch_all(db())
    .await?;

    let certificates = rows
        .into_iter()
        .filter_map(|row| {
            Some(CertificateInfo {
                certificate_id: row.certificate_id.parse().ok()?,
                created_at: row.created_at,
                user_id: row.user_id.parse().ok()?,
                project: row.project,
                mode: row.mode,
            })
        })
        .collect();

    Ok(UserCertificatesResponse { certificates })
}

pub async fn get(req: &Request) -> Result<CertificateGetResponse, Error> {
    let input: CertificateGetRequest = flexbuffers::from_slice(&req.message.payload)?;

    let cert_id_str = input.certificate_id.to_string();

    struct CertRow {
        certificate_id: String,
        created_at: String,
        user_id: String,
        project: String,
        mode: String,
    }

    let row = sqlx::query_as!(
        CertRow,
        "SELECT certificate_id, created_at, user_id, project, mode FROM certificates WHERE certificate_id = ?",
        cert_id_str,
    )
    .fetch_optional(db())
    .await?
    .ok_or_else(|| anyhow::anyhow!("Certificate not found"))?;

    struct UserRow {
        handle: String,
        first_name: String,
        last_name: String,
    }

    let user = sqlx::query_as!(
        UserRow,
        r#"SELECT handle, first_name, last_name FROM users WHERE user_id = ?"#,
        row.user_id,
    )
    .fetch_optional(db())
    .await?
    .unwrap_or(UserRow {
        handle: String::new(),
        first_name: String::new(),
        last_name: String::new(),
    });

    let display_name = if user.first_name.is_empty() && user.last_name.is_empty() {
        user.handle
    } else {
        format!("{} {}", user.first_name, user.last_name)
            .trim()
            .to_string()
    };

    Ok(CertificateGetResponse {
        certificate_id: row.certificate_id.parse()?,
        created_at: row.created_at,
        user_id: row.user_id.parse()?,
        project: row.project,
        mode: row.mode,
        display_name,
    })
}
