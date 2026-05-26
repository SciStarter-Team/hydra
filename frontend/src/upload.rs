use anyhow::Result;
use poem::{
    Body, Endpoint, IntoResponse, Response, Route, get, handler,
    http::{StatusCode, header::CONTENT_TYPE},
    post, put,
    web::{Form, Json, Multipart, Path, Query},
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};
use tokio::io::AsyncReadExt;
use tracing::error;
use ulid::Ulid;

use crate::admin::UploadSuccessResponse;
use crate::gourl::ProgramQuery;
use crate::middleware::authorization::Authenticated;
use common::service::log_activity;
use sailfish::TemplateOnce;

pub mod processing;
pub mod storage;

use storage::{FileAccess, FileMetadata, StorageManager};

type SharedStorage = Arc<StorageManager>;

pub static STORAGE: LazyLock<SharedStorage> = LazyLock::new(|| {
    Arc::new(StorageManager::new().expect("Failed to initialize storage manager"))
});

pub fn authenticated_routes() -> impl Endpoint {
    Route::new()
        .at("/tracked", post(upload_tracked_file))
        .at("/file", post(upload_untracked_file))
        .at("/files/:file_id", get(get_tracked_file))
        .at("/files/:file_id/friendly-url", put(set_friendly_url))
        .at("/files/:file_id/download-mode", put(set_download_mode))
}

pub fn anonymous_routes() -> impl Endpoint {
    Route::new().at("/:file_id", get(get_untracked_file))
}

#[handler]
async fn upload_tracked_file(
    Authenticated(user_info): Authenticated,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, poem::Error> {
    let user_info = user_info.ok_or_else(|| {
        poem::Error::from_string("Authentication required", StatusCode::UNAUTHORIZED)
    })?;

    let mut file_data: Option<(String, Option<String>, Vec<u8>)> = None; // (filename, content_type, data)
    let mut slug: Option<String> = None;

    // Process all multipart fields
    while let Some(field) = multipart.next_field().await? {
        let name = field.name().map(|s| s.to_string());

        if let Some(field_name) = name {
            match field_name.as_str() {
                "file" => {
                    let filename = field.file_name().map(|s| s.to_string());
                    let content_type = field.content_type().map(|s| s.to_string());

                    let mut data = Vec::new();
                    let mut reader = field.into_async_read();
                    reader.read_to_end(&mut data).await.map_err(|e| {
                        error!("Failed to read uploaded file: {}", e);
                        poem::Error::from_string(
                            "Failed to read file",
                            StatusCode::INTERNAL_SERVER_ERROR,
                        )
                    })?;

                    file_data = Some((
                        filename.unwrap_or_else(|| "unknown".to_string()),
                        content_type,
                        data,
                    ));
                }
                "slug" => {
                    let mut slug_data = Vec::new();
                    let mut reader = field.into_async_read();
                    reader.read_to_end(&mut slug_data).await.map_err(|e| {
                        error!("Failed to read slug field: {}", e);
                        poem::Error::from_string(
                            "Failed to read slug",
                            StatusCode::INTERNAL_SERVER_ERROR,
                        )
                    })?;
                    slug = Some(String::from_utf8(slug_data).map_err(|e| {
                        error!("Invalid slug encoding: {}", e);
                        poem::Error::from_string("Invalid slug", StatusCode::BAD_REQUEST)
                    })?);
                }
                _ => {} // Ignore unknown fields
            }
        }
    }

    // Validate required fields
    let (filename, content_type, data) = file_data
        .ok_or_else(|| poem::Error::from_string("No file provided", StatusCode::BAD_REQUEST))?;

    let slug =
        slug.ok_or_else(|| poem::Error::from_string("No slug provided", StatusCode::BAD_REQUEST))?;

    // Validate slug format
    if !slug.chars().all(|c| c.is_alphanumeric() || c == '-') || slug.is_empty() {
        return Err(poem::Error::from_string(
            "Slug must contain only alphanumeric characters and hyphens",
            StatusCode::BAD_REQUEST,
        ));
    }

    let file_id = Ulid::new();

    let metadata = FileMetadata {
        file_id,
        original_filename: Some(filename),
        content_type,
        size: data.len() as u64,
        uploaded_by: user_info.user_id,
        access: FileAccess::Tracked,
        variant: None,         // Original upload, no variant
        force_download: false, // Default to letting browser decide
    };

    // Upload the file
    match STORAGE.upload_file(metadata, data).await {
        Ok(_file_path) => {
            // Create the go URL
            use crate::gourl::GoUrl;
            use crate::peering::LoadStorePeered;

            let go_url = GoUrl::new_file_url(slug.clone(), file_id);

            // Save the go URL to database
            match go_url.save_to_database().await {
                Ok(()) => {
                    // Get updated go URLs list for OOB update
                    let go_urls_list =
                        crate::admin::get_go_urls_with_file_info()
                            .await
                            .map_err(|e| {
                                error!("Failed to get go URLs: {}", e);
                                poem::Error::from_string(
                                    "Failed to get go URLs",
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                )
                            })?;

                    let response = UploadSuccessResponse {
                        slug,
                        file_id,
                        go_urls_list,
                    };

                    let rendered = response.render_once().map_err(|e| {
                        error!("Failed to render upload success template: {}", e);
                        poem::Error::from_string(
                            "Failed to render response",
                            StatusCode::INTERNAL_SERVER_ERROR,
                        )
                    })?;

                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .content_type("text/html")
                        .body(rendered))
                }
                Err(e) => {
                    error!("Failed to create go URL for file {}: {}", file_id, e);
                    // File was uploaded but go URL creation failed
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .content_type("text/html")
                        .body(format!(
                            r#"<div class="text-yellow-600 font-medium">
                                File uploaded successfully, but go URL creation failed.
                                <span class="text-sm text-gray-600">File ID: {} | Error: {}</span>
                            </div>"#,
                            file_id, e
                        )))
                }
            }
        }
        Err(e) => {
            error!("Failed to upload tracked file: {:#}", e);
            Err(poem::Error::from_string(
                "Upload failed",
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

#[handler]
async fn upload_untracked_file(
    Authenticated(user_info): Authenticated,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, poem::Error> {
    let user_info = user_info.ok_or_else(|| {
        poem::Error::from_string("Authentication required", StatusCode::UNAUTHORIZED)
    })?;

    while let Some(field) = multipart.next_field().await? {
        let name = field.name().map(|s| s.to_string());

        if let Some(field_name) = name {
            if field_name == "file" {
                let filename = field.file_name().map(|s| s.to_string());
                let content_type = field.content_type().map(|s| s.to_string());

                let file_id = Ulid::new();
                let mut data = Vec::new();
                let mut reader = field.into_async_read();
                reader.read_to_end(&mut data).await.map_err(|e| {
                    error!("Failed to read uploaded file: {}", e);
                    poem::Error::from_string(
                        "Failed to read file",
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;

                let metadata = FileMetadata {
                    file_id,
                    original_filename: filename,
                    content_type,
                    size: data.len() as u64,
                    uploaded_by: user_info.user_id,
                    access: FileAccess::Untracked,
                    variant: None,         // Original upload, no variant
                    force_download: false, // Default to letting browser decide
                };

                match STORAGE.upload_file(metadata, data).await {
                    Ok(file_path) => {
                        return Ok(Response::builder()
                            .status(StatusCode::OK)
                            .content_type("application/json")
                            .body(
                                serde_json::to_string(&serde_json::json!({
                                    "file_id": file_id.to_string(),
                                    "file_path": file_path,
                                    "status": "uploaded"
                                }))
                                .map_err(|e| {
                                    poem::Error::from_string(
                                        e.to_string(),
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                    )
                                })?,
                            ));
                    }
                    Err(e) => {
                        error!("Failed to upload untracked file: {:#}", e);
                        return Err(poem::Error::from_string(
                            "Upload failed",
                            StatusCode::INTERNAL_SERVER_ERROR,
                        ));
                    }
                }
            }
        }
    }

    Err(poem::Error::from_string(
        "No file provided",
        StatusCode::BAD_REQUEST,
    ))
}

#[handler]
async fn get_tracked_file(
    Authenticated(user_info): Authenticated,
    Path(file_id): Path<String>,
    Query(query): Query<ProgramQuery>,
) -> Result<impl IntoResponse, poem::Error> {
    let file_id: Ulid = file_id
        .parse()
        .map_err(|_| poem::Error::from_string("Invalid file ID", StatusCode::BAD_REQUEST))?;

    match STORAGE
        .get_file(file_id, user_info.as_ref().map(|u| u.user_id))
        .await
    {
        Ok((metadata, data)) => {
            // Log the file access
            if let Err(e) = log_activity(
                None,
                "file-view",
                user_info.as_ref().map(|u| u.user_id),
                Some(file_id),         // File ID goes in target field
                query.program.clone(), // Program ID goes in extra field
            )
            .await
            {
                error!("Failed to log file view: {}", e);
            }

            let content_type = metadata
                .content_type
                .unwrap_or_else(|| "application/octet-stream".to_string());

            let mut response = Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, content_type);

            // Add attachment headers if force_download is true
            if metadata.force_download {
                response = response.header(
                    "Content-Disposition",
                    if let Some(filename) = &metadata.original_filename {
                        format!("attachment; filename=\"{}\"", filename)
                    } else {
                        "attachment".to_string()
                    },
                );
            }

            Ok(response.body(Body::from_bytes(data.into())))
        }
        Err(e) => {
            error!("Failed to retrieve tracked file {}: {}", file_id, e);
            Err(poem::Error::from_string(
                "File not found",
                StatusCode::NOT_FOUND,
            ))
        }
    }
}

#[handler]
async fn get_untracked_file(Path(file_id): Path<String>) -> Result<impl IntoResponse, poem::Error> {
    let file_id: Ulid = file_id
        .parse()
        .map_err(|_| poem::Error::from_string("Invalid file ID", StatusCode::BAD_REQUEST))?;

    match STORAGE.get_file(file_id, None).await {
        Ok((metadata, data)) => {
            let content_type = metadata
                .content_type
                .unwrap_or_else(|| "application/octet-stream".to_string());

            let mut response = Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, content_type);

            // Add attachment headers if force_download is true
            if metadata.force_download {
                response = response.header(
                    "Content-Disposition",
                    if let Some(filename) = &metadata.original_filename {
                        format!("attachment; filename=\"{}\"", filename)
                    } else {
                        "attachment".to_string()
                    },
                );
            }

            Ok(response.body(Body::from_bytes(data.into())))
        }
        Err(e) => {
            error!("Failed to retrieve untracked file {}: {}", file_id, e);
            Err(poem::Error::from_string(
                "File not found",
                StatusCode::NOT_FOUND,
            ))
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SetFriendlyUrlRequest {
    friendly_url: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct SetDownloadModeRequest {
    force_download: bool,
    slug: String,
}

#[handler]
async fn set_friendly_url(
    Authenticated(user_info): Authenticated,
    Path(file_id): Path<String>,
    Json(request): Json<SetFriendlyUrlRequest>,
) -> Result<impl IntoResponse, poem::Error> {
    let user_info = user_info.ok_or_else(|| {
        poem::Error::from_string("Authentication required", StatusCode::UNAUTHORIZED)
    })?;

    let file_id: Ulid = file_id
        .parse()
        .map_err(|_| poem::Error::from_string("Invalid file ID", StatusCode::BAD_REQUEST))?;

    match STORAGE
        .set_friendly_url(file_id, request.friendly_url.clone(), &Some(user_info))
        .await
    {
        Ok(()) => Ok(Response::builder()
            .status(StatusCode::OK)
            .content_type("application/json")
            .body(
                serde_json::to_string(&serde_json::json!({
                    "status": "success",
                    "friendly_url": request.friendly_url
                }))
                .map_err(|e| {
                    poem::Error::from_string(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
                })?,
            )),
        Err(e) => {
            error!("Failed to set friendly URL for file {}: {}", file_id, e);
            Err(poem::Error::from_string(
                e.to_string(),
                StatusCode::BAD_REQUEST,
            ))
        }
    }
}

#[handler]
async fn set_download_mode(
    Authenticated(user_info): Authenticated,
    Path(file_id): Path<String>,
    Form(request): Form<SetDownloadModeRequest>,
) -> Result<impl IntoResponse, poem::Error> {
    let user_info = user_info.ok_or_else(|| {
        poem::Error::from_string("Authentication required", StatusCode::UNAUTHORIZED)
    })?;

    let file_id: Ulid = file_id
        .parse()
        .map_err(|_| poem::Error::from_string("Invalid file ID", StatusCode::BAD_REQUEST))?;

    match STORAGE
        .set_download_mode(file_id, request.force_download, &Some(user_info))
        .await
    {
        Ok(()) => {
            // Use the slug from the form to get the go URL and render the updated row
            use crate::gourl::GoUrl;
            use crate::peering::LoadStorePeered;

            match GoUrl::slug_exists(&request.slug).await {
                Ok(false) => {
                    // Create new go URL since it doesn't exist
                    let go_url = GoUrl::new_file_url(request.slug.clone(), file_id);
                    if let Err(e) = go_url.store().await {
                        tracing::error!("Failed to create go URL: {}", e);
                        return Ok(poem::Response::builder()
                            .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
                            .content_type("text/html")
                            .body("Failed to create go URL"));
                    }
                }
                Ok(true) => {
                    // Go URL already exists, we'll load it below to get the correct case
                }
                Err(e) => {
                    tracing::error!("Failed to check slug existence: {}", e);
                    return Ok(poem::Response::builder()
                        .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
                        .content_type("text/html")
                        .body("Database error"));
                }
            }

            match GoUrl::load(request.slug.clone()).await {
                Ok(go_url) => {
                    // Get the updated file metadata
                    use crate::upload::storage::FileMetadata;

                    match FileMetadata::load(file_id).await {
                        Ok(file_metadata) => {
                            use crate::admin::{GoUrlRow, GoUrlWithFileInfo};
                            use sailfish::TemplateOnce;

                            let go_url_with_info = GoUrlWithFileInfo {
                                slug: go_url.slug,
                                url_type: go_url.url_type,
                                file_info: Some(file_metadata),
                            };

                            let row_template = GoUrlRow {
                                go_url: &go_url_with_info,
                                oob: false,
                                oob_swap: "outerHTML".to_string(),
                            };

                            match row_template.render_once() {
                                Ok(rendered) => Ok(Response::builder()
                                    .status(StatusCode::OK)
                                    .content_type("text/html")
                                    .body(rendered)),
                                Err(e) => {
                                    error!("Failed to render go URL row: {}", e);
                                    Err(poem::Error::from_string(
                                        "Failed to render updated row",
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                    ))
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to load file metadata for {}: {}", file_id, e);
                            Err(poem::Error::from_string(
                                "Failed to load file data",
                                StatusCode::INTERNAL_SERVER_ERROR,
                            ))
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to load go URL {}: {}", request.slug, e);
                    Err(poem::Error::from_string(
                        "Go URL not found",
                        StatusCode::NOT_FOUND,
                    ))
                }
            }
        }
        Err(e) => {
            error!("Failed to set download mode for file {}: {}", file_id, e);
            Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .content_type("text/html")
                .body(format!(r#"<div class="text-red-600">Error: {}</div>"#, e)))
        }
    }
}
