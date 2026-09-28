use crate::peering::LoadStorePeered;
use anyhow::{Context, Result};
use object_store::{ObjectStore, aws::AmazonS3Builder, path::Path as ObjectPath};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{error, info};
use ulid::Ulid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileAccess {
    Tracked,
    Untracked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub file_id: Ulid,
    pub original_filename: Option<String>,
    pub content_type: Option<String>,
    pub size: u64,
    pub uploaded_by: Ulid,
    pub access: FileAccess,
    pub variant: Option<String>,
    pub force_download: bool, // Whether to serve with attachment headers
}

/// Where untracked (public) uploads are served: `<UNTRACKED_PATH>/<file id>`.
pub const UNTRACKED_PATH: &str = "/-uploaded";

/// The file id in an untracked upload's public path, if `path` is one.
pub fn untracked_file_id(path: &str) -> Option<Ulid> {
    path.strip_prefix(UNTRACKED_PATH)?
        .strip_prefix('/')?
        .parse()
        .ok()
}

impl FileMetadata {
    /// Where the file's bytes live in the uploads bucket:
    /// `<sanitized filename stem>/<file id>.<extension>`, or `unnamed/<file
    /// id>.bin` when the upload had no name. Storing and fetching both address
    /// the object through this, and so does anything that reads the bucket
    /// directly, such as the thumbnail service (see [`crate::thumbnail`]).
    pub fn object_key(&self) -> String {
        let name = self.original_filename.as_deref().map(Path::new);
        let extension = name
            .and_then(Path::extension)
            .and_then(|ext| ext.to_str())
            .unwrap_or("bin");
        let folder = name
            .map(|name| {
                sanitize_filename(
                    name.file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("unnamed"),
                )
            })
            .unwrap_or_else(|| "unnamed".to_string());
        format!("{folder}/{}.{extension}", self.file_id)
    }
}

pub struct StorageManager {
    object_store: Box<dyn ObjectStore>,
}

impl StorageManager {
    pub fn new() -> Result<Self> {
        let host = env!("OBJECT_STORAGE_HOST");
        let key = env!("OBJECT_STORAGE_KEY");
        let secret = env!("OBJECT_STORAGE_SECRET");

        let object_store = AmazonS3Builder::new()
            .with_endpoint(host)
            .with_access_key_id(key)
            .with_secret_access_key(secret)
            .with_bucket_name("uploads")
            .with_region("us-east-1") // Linode typically uses us-east-1
            .build()
            .context("Failed to create S3 client")?;

        Ok(Self {
            object_store: Box::new(object_store),
        })
    }

    pub async fn upload_file(&self, metadata: FileMetadata, data: Vec<u8>) -> Result<String> {
        use super::processing::ProcessingPipeline;

        let pipeline = ProcessingPipeline::new();
        let processed_files = pipeline.process_file(&metadata, data).await?;

        if processed_files.is_empty() {
            return Err(anyhow::anyhow!("File was rejected by processing pipeline"));
        }

        // Store all processed files
        for processed_file in &processed_files {
            self.store_processed_file(processed_file, &metadata).await?;
        }

        // Return the path for the original file (first in the list)
        let file_path = match metadata.access {
            FileAccess::Tracked => format!("/-upload/files/{}", metadata.file_id),
            FileAccess::Untracked => format!("{UNTRACKED_PATH}/{}", metadata.file_id),
        };

        Ok(file_path)
    }

    async fn store_processed_file(
        &self,
        processed_file: &super::processing::ProcessedFile,
        original_metadata: &FileMetadata,
    ) -> Result<()> {
        let file_metadata = processed_file.to_metadata(
            original_metadata.uploaded_by,
            original_metadata.access.clone(),
        );
        let object_path = file_metadata.object_key();
        let object_key = ObjectPath::from(object_path.clone());

        // Store the file in object storage
        self.object_store
            .put(&object_key, processed_file.data.clone().into())
            .await
            .with_context(|| format!("Failed to upload file to object storage: {}", object_path))?;

        // Store metadata using LoadStorePeered
        file_metadata.store().await?;

        info!(
            "Uploaded processed file {} to {} (variant: {:?})",
            processed_file.file_id, object_path, processed_file.variant
        );
        Ok(())
    }

    pub async fn get_file(
        &self,
        file_id: Ulid,
        requesting_user: Option<Ulid>,
    ) -> Result<(FileMetadata, Vec<u8>)> {
        // Get metadata using LoadStorePeered
        let metadata = FileMetadata::load(file_id).await?;

        // Check access permissions
        match metadata.access {
            FileAccess::Tracked => {
                if let Some(user_id) = requesting_user {
                    if metadata.uploaded_by != user_id {
                        return Err(anyhow::anyhow!(
                            "Access denied: file belongs to different user"
                        ));
                    }
                } else {
                    return Err(anyhow::anyhow!("Authentication required for tracked files"));
                }
            }
            FileAccess::Untracked => {
                // Untracked files can be accessed by anyone
            }
        }

        let object_path = metadata.object_key();
        let object_key = ObjectPath::from(object_path.clone());

        // Get the file from object storage
        let data = self
            .object_store
            .get(&object_key)
            .await
            .with_context(|| {
                format!(
                    "Failed to retrieve file from object storage: {}",
                    object_path
                )
            })?
            .bytes()
            .await
            .context("Failed to read file bytes")?
            .to_vec();

        Ok((metadata, data))
    }

    pub async fn set_friendly_url(
        &self,
        file_id: Ulid,
        friendly_url: Option<String>,
        requesting_user: &Option<std::sync::Arc<common::message::user::BasicInfo>>,
    ) -> Result<()> {
        use crate::gourl::GoUrl;
        use common::service::log_activity;

        // Check if the user is staff
        let user_info = requesting_user
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Authentication required to set friendly URLs"))?;

        if !user_info.is_staff {
            return Err(anyhow::anyhow!("Only staff users can set friendly URLs"));
        }

        // Verify it's a tracked file
        let metadata = FileMetadata::load(file_id).await?;
        if !matches!(metadata.access, FileAccess::Tracked) {
            return Err(anyhow::anyhow!(
                "Friendly URLs can only be set for tracked files"
            ));
        }

        match friendly_url {
            Some(slug) => {
                // Validate friendly URL format
                if !GoUrl::is_valid_slug(&slug) {
                    return Err(anyhow::anyhow!(
                        "Invalid friendly URL format. Use only letters, numbers, and hyphens."
                    ));
                }

                // Create or update the GoUrl entry
                let go_url = GoUrl::new_file_url(slug.clone(), file_id);
                go_url.store().await?;

                // Log the creation/update
                if let Err(e) = log_activity(
                    None,
                    "friendly-url-set",
                    Some(user_info.user_id),
                    Some(file_id),
                    Some(format!("/go/{}", slug)),
                )
                .await
                {
                    error!("Failed to log friendly URL creation: {}", e);
                }
            }
            None => {
                // TODO: Remove existing friendly URL
                // We'd need to find and delete the GoUrl entry that points to this file_id
                // This requires a reverse lookup capability we haven't implemented yet
                return Err(anyhow::anyhow!(
                    "Removing friendly URLs not yet implemented"
                ));
            }
        }

        Ok(())
    }

    pub async fn set_download_mode(
        &self,
        file_id: Ulid,
        force_download: bool,
        requesting_user: &Option<std::sync::Arc<common::message::user::BasicInfo>>,
    ) -> Result<()> {
        use common::service::log_activity;

        // Check if the user is staff
        let user_info = requesting_user
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Authentication required to set download mode"))?;

        if !user_info.is_staff {
            return Err(anyhow::anyhow!("Only staff users can set download mode"));
        }

        // Verify the file exists and get its metadata
        let _metadata = FileMetadata::load(file_id).await?;

        // Update the force_download flag in the database
        use common::shared::db;
        let file_id_string = file_id.to_string();
        sqlx::query!(
            r#"UPDATE uploaded_files SET force_download = ? WHERE file_id = ?"#,
            force_download,
            file_id_string
        )
        .execute(db())
        .await
        .context("Failed to update download mode in database")?;

        // Log the action
        if let Err(e) = log_activity(
            None,
            "download-mode-set",
            Some(user_info.user_id),
            Some(file_id),
            Some(format!("force_download={}", force_download)),
        )
        .await
        {
            error!("Failed to log download mode change: {}", e);
        }

        Ok(())
    }
}

#[derive(Debug)]
struct FileMetadataRow {
    file_id: String,
    original_filename: Option<String>,
    content_type: Option<String>,
    size: i64,
    uploaded_by: String,
    access: String,
    #[allow(dead_code)]
    created_at: String,
    variant: Option<String>,
    force_download: bool,
}

impl TryFrom<FileMetadataRow> for FileMetadata {
    type Error = anyhow::Error;

    fn try_from(row: FileMetadataRow) -> Result<Self> {
        let file_id = row.file_id.parse().context("Invalid file_id ULID")?;

        let uploaded_by = row
            .uploaded_by
            .parse()
            .context("Invalid uploaded_by ULID")?;

        let access = match row.access.as_str() {
            "tracked" => FileAccess::Tracked,
            "untracked" => FileAccess::Untracked,
            _ => return Err(anyhow::anyhow!("Invalid access type: {}", row.access)),
        };

        Ok(FileMetadata {
            file_id,
            original_filename: row.original_filename,
            content_type: row.content_type,
            size: row.size as u64,
            uploaded_by,
            access,
            variant: row.variant,
            force_download: row.force_download,
        })
    }
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => c,
            ' ' => '_',
            _ => '_',
        })
        .collect()
}

impl LoadStorePeered for FileMetadata {
    type DatabaseKey = Ulid;
    const GROUP: &str = "file";
    const SPECIFIC: &str = "metadata";

    async fn save_to_database(&self) -> Result<(), anyhow::Error> {
        use common::shared::db;

        let access_str = match self.access {
            FileAccess::Tracked => "tracked",
            FileAccess::Untracked => "untracked",
        };

        let file_id_string = self.file_id.to_string();
        let uploaded_by_string = self.uploaded_by.to_string();
        let size_i64 = self.size as i64;
        sqlx::query!(
            r#"INSERT OR REPLACE INTO uploaded_files (file_id, original_filename, content_type, size, uploaded_by, access, created_at, variant, force_download)
               VALUES (?, ?, ?, ?, ?, ?, datetime('now'), ?, ?)"#,
            file_id_string,
            self.original_filename,
            self.content_type,
            size_i64,
            uploaded_by_string,
            access_str,
            self.variant,
            self.force_download
        )
        .execute(db())
        .await
        .context("Failed to save file metadata to database")?;

        Ok(())
    }

    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, anyhow::Error> {
        use common::shared::db;

        let key_string = key.to_string();
        let row = sqlx::query_as!(
            FileMetadataRow,
            r#"SELECT file_id, original_filename, content_type, size, uploaded_by, access, created_at, variant, force_download
               FROM uploaded_files
               WHERE file_id = ?"#,
            key_string
        )
        .fetch_optional(db())
        .await
        .context("Failed to query file metadata from database")?
        .ok_or_else(|| anyhow::anyhow!("File metadata not found"))?;

        row.try_into()
    }
}
