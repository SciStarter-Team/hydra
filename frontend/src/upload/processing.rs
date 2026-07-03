use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use ulid::Ulid;

use super::storage::{FileAccess, FileMetadata};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessedFile {
    pub file_id: Ulid,
    pub original_filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
    pub size: u64,
    pub variant: Option<String>, // e.g., "thumbnail", "medium", "large", "webp_version"
}

impl ProcessedFile {
    pub fn new(data: Vec<u8>, variant: Option<String>) -> Self {
        Self {
            file_id: Ulid::new(),
            original_filename: None,
            content_type: None,
            size: data.len() as u64,
            data,
            variant,
        }
    }

    pub fn with_metadata(mut self, original: &FileMetadata) -> Self {
        self.original_filename = original.original_filename.clone();
        self.content_type = original.content_type.clone();
        self
    }

    pub fn with_filename_suffix(mut self, suffix: &str) -> Self {
        if let Some(ref filename) = self.original_filename {
            if let Some(dot_pos) = filename.rfind('.') {
                let (name, ext) = filename.split_at(dot_pos);
                self.original_filename = Some(format!("{}{}{}", name, suffix, ext));
            } else {
                self.original_filename = Some(format!("{}{}", filename, suffix));
            }
        }
        self
    }

    pub fn with_content_type(mut self, content_type: &str) -> Self {
        self.content_type = Some(content_type.to_string());
        self
    }

    pub fn to_metadata(&self, uploaded_by: Ulid, access: FileAccess) -> FileMetadata {
        FileMetadata {
            file_id: self.file_id,
            original_filename: self.original_filename.clone(),
            content_type: self.content_type.clone(),
            size: self.size,
            uploaded_by,
            access,
            variant: self.variant.clone(),
            force_download: false, // Processed files default to browser decision
        }
    }
}

#[derive(Debug)]
pub enum FileProcessor {
    Security(SecurityFilter),
    Svg(SvgSanitizer),
    Image(ImageProcessor),
    Document(DocumentProcessor),
}

impl FileProcessor {
    fn can_process(&self, metadata: &FileMetadata) -> bool {
        match self {
            FileProcessor::Security(p) => p.can_process(metadata),
            FileProcessor::Svg(p) => p.can_process(metadata),
            FileProcessor::Image(p) => p.can_process(metadata),
            FileProcessor::Document(p) => p.can_process(metadata),
        }
    }

    async fn process(&self, metadata: &FileMetadata, data: &[u8]) -> Result<Vec<ProcessedFile>> {
        match self {
            FileProcessor::Security(p) => p.process(metadata, data).await,
            FileProcessor::Svg(p) => p.process(metadata, data).await,
            FileProcessor::Image(p) => p.process(metadata, data).await,
            FileProcessor::Document(p) => p.process(metadata, data).await,
        }
    }
}

#[derive(Debug)]
pub struct SvgSanitizer;

impl SvgSanitizer {
    fn can_process(&self, metadata: &FileMetadata) -> bool {
        if let Some(content_type) = &metadata.content_type {
            let ct = content_type
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if ct == "image/svg+xml" {
                return true;
            }
        }
        metadata
            .original_filename
            .as_ref()
            .is_some_and(|f| f.to_ascii_lowercase().ends_with(".svg"))
    }

    async fn process(&self, metadata: &FileMetadata, data: &[u8]) -> Result<Vec<ProcessedFile>> {
        info!("Sanitizing SVG upload: {}", metadata.file_id);

        // SVGs can carry active content (scripts, event handlers, external
        // references). Rewrite the file through an allowlist-based SVG sanitizer
        // so the stored copy is inert and safe to serve inline. The XML parse +
        // rewrite is CPU-bound, so run it off the async executor.
        let input = data.to_vec();
        let sanitized = tokio::task::spawn_blocking(move || {
            use svg_hush::{Filter, data_url_filter};
            let mut filter = Filter::new();
            // Keep standard raster image `data:` URLs so embedded bitmaps still
            // render, while scripts/handlers/external references are dropped.
            filter.set_data_url_filter(data_url_filter::allow_standard_images);
            let mut out = Vec::new();
            filter.filter(input.as_slice(), &mut out).map(|()| out)
        })
        .await;

        // On any failure, reject the upload by returning an empty vec. We must
        // never fall through to storing the unsanitized bytes — returning `Err`
        // here would do exactly that, because the pipeline keeps the original
        // file when a processor errors.
        let sanitized = match sanitized {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(e)) => {
                warn!(
                    "Rejected SVG {}: could not sanitize: {}",
                    metadata.file_id, e
                );
                return Ok(vec![]);
            }
            Err(e) => {
                warn!(
                    "Rejected SVG {}: sanitizer task failed: {}",
                    metadata.file_id, e
                );
                return Ok(vec![]);
            }
        };

        // Normalize the stored content type to SVG so the serving side treats it
        // as the sanitized image it now is.
        let mut processed = ProcessedFile::new(sanitized, None)
            .with_metadata(metadata)
            .with_content_type("image/svg+xml");
        processed.file_id = metadata.file_id;
        Ok(vec![processed])
    }
}

#[derive(Debug)]
pub struct ImageProcessor;

impl ImageProcessor {
    fn can_process(&self, metadata: &FileMetadata) -> bool {
        if let Some(content_type) = &metadata.content_type {
            content_type.starts_with("image/")
        } else if let Some(filename) = &metadata.original_filename {
            let lower = filename.to_lowercase();
            lower.ends_with(".jpg")
                || lower.ends_with(".jpeg")
                || lower.ends_with(".png")
                || lower.ends_with(".gif")
                || lower.ends_with(".webp")
        } else {
            false
        }
    }

    async fn process(&self, metadata: &FileMetadata, data: &[u8]) -> Result<Vec<ProcessedFile>> {
        info!(
            "Processing image file: {} ({})",
            metadata.file_id,
            metadata.original_filename.as_deref().unwrap_or("unknown")
        );

        // For now, just return the original file
        // In the future, this could:
        // - Create multiple sizes (thumbnail, medium, large)
        // - Convert formats (original + WebP version)
        // - Strip EXIF data
        // - Optimize file size

        let mut original = ProcessedFile::new(data.to_vec(), None).with_metadata(metadata);
        // Preserve the original file ID for the main file
        original.file_id = metadata.file_id;

        // Future example of creating multiple variants:
        // let thumbnail = ProcessedFile::new(create_thumbnail(data)?, Some("thumbnail".to_string()))
        //     .with_metadata(metadata)
        //     .with_filename_suffix("_thumb");
        //
        // let webp_version = ProcessedFile::new(convert_to_webp(data)?, Some("webp".to_string()))
        //     .with_metadata(metadata)
        //     .with_content_type("image/webp");
        //
        // Ok(vec![original, thumbnail, webp_version])

        Ok(vec![original])
    }
}

#[derive(Debug)]
pub struct DocumentProcessor;

impl DocumentProcessor {
    fn can_process(&self, metadata: &FileMetadata) -> bool {
        if let Some(content_type) = &metadata.content_type {
            content_type == "application/pdf"
                || content_type.starts_with("text/")
                || content_type == "application/msword"
                || content_type
                    == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        } else if let Some(filename) = &metadata.original_filename {
            let lower = filename.to_lowercase();
            lower.ends_with(".pdf")
                || lower.ends_with(".txt")
                || lower.ends_with(".doc")
                || lower.ends_with(".docx")
        } else {
            false
        }
    }

    async fn process(&self, metadata: &FileMetadata, data: &[u8]) -> Result<Vec<ProcessedFile>> {
        info!(
            "Processing document file: {} ({})",
            metadata.file_id,
            metadata.original_filename.as_deref().unwrap_or("unknown")
        );

        // Future document processing could:
        // - Extract text content as a separate file
        // - Create PDF preview images
        // - Scan for malware and return empty vec if infected
        // - Convert to different formats

        let mut original = ProcessedFile::new(data.to_vec(), None).with_metadata(metadata);
        // Preserve the original file ID for the main file
        original.file_id = metadata.file_id;

        Ok(vec![original])
    }
}

#[derive(Debug)]
pub struct SecurityFilter;

impl SecurityFilter {
    fn can_process(&self, _metadata: &FileMetadata) -> bool {
        // This processor can handle any file type for security scanning
        true
    }

    async fn process(&self, metadata: &FileMetadata, data: &[u8]) -> Result<Vec<ProcessedFile>> {
        info!(
            "Security scanning file: {} ({})",
            metadata.file_id,
            metadata.original_filename.as_deref().unwrap_or("unknown")
        );

        // Placeholder for security checks:
        // - Virus scanning
        // - Content analysis
        // - File size limits
        // - Blocked file types

        // Example: block files over 100MB
        if data.len() > 100 * 1024 * 1024 {
            warn!(
                "File {} rejected: too large ({} bytes)",
                metadata.file_id,
                data.len()
            );
            return Ok(vec![]); // Return empty vec to block upload
        }

        // Example: block certain extensions
        if let Some(filename) = &metadata.original_filename {
            let lower = filename.to_lowercase();
            if lower.ends_with(".exe") || lower.ends_with(".bat") || lower.ends_with(".sh") {
                warn!("File {} rejected: dangerous file type", metadata.file_id);
                return Ok(vec![]);
            }
        }

        // If all checks pass, return the original file
        let mut processed = ProcessedFile::new(data.to_vec(), None).with_metadata(metadata);
        // Preserve the original file ID for the main file
        processed.file_id = metadata.file_id;

        Ok(vec![processed])
    }
}

pub struct ProcessingPipeline {
    processors: Vec<FileProcessor>,
}

impl ProcessingPipeline {
    pub fn new() -> Self {
        Self {
            processors: vec![
                FileProcessor::Security(SecurityFilter), // Security filter runs first
                FileProcessor::Svg(SvgSanitizer),        // Sanitize SVGs before storage
                FileProcessor::Image(ImageProcessor),
                FileProcessor::Document(DocumentProcessor),
            ],
        }
    }

    pub async fn process_file(
        &self,
        metadata: &FileMetadata,
        data: Vec<u8>,
    ) -> Result<Vec<ProcessedFile>> {
        let mut initial_file = ProcessedFile::new(data, None).with_metadata(metadata);
        // Preserve the original file ID for the main file
        initial_file.file_id = metadata.file_id;
        let mut files_to_store = vec![initial_file];

        for processor in &self.processors {
            if processor.can_process(metadata) {
                let mut new_files = Vec::new();

                for file in files_to_store {
                    match processor.process(metadata, &file.data).await {
                        Ok(mut processed_files) => {
                            if processed_files.is_empty() {
                                info!("File {} filtered out by processor", metadata.file_id);
                                // If any processor returns empty, the file is rejected
                                return Ok(vec![]);
                            }
                            new_files.append(&mut processed_files);
                        }
                        Err(e) => {
                            warn!(
                                "Failed to process file {} with processor: {}",
                                metadata.file_id, e
                            );
                            // On error, keep the original file
                            new_files.push(file);
                        }
                    }
                }

                files_to_store = new_files;
            }
        }

        info!(
            "Processing pipeline completed for file {}: {} files to store",
            metadata.file_id,
            files_to_store.len()
        );

        Ok(files_to_store)
    }
}

impl Default for ProcessingPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use svg_hush::{Filter, data_url_filter};

    /// Sanitize exactly as [`SvgSanitizer::process`] does.
    fn sanitize(svg: &[u8]) -> String {
        let mut filter = Filter::new();
        filter.set_data_url_filter(data_url_filter::allow_standard_images);
        let mut out = Vec::new();
        filter.filter(svg, &mut out).expect("filter should succeed");
        String::from_utf8(out).expect("output is utf-8")
    }

    #[test]
    fn strips_active_content() {
        let malicious = br#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)">
            <script>alert(document.cookie)</script>
            <a href="javascript:alert(2)"><rect width="10" height="10"/></a>
            <image href="http://169.254.169.254/latest/meta-data/"/>
            <foreignObject><body xmlns="http://www.w3.org/1999/xhtml"><script>alert(3)</script></body></foreignObject>
        </svg>"#;
        let out = sanitize(malicious).to_ascii_lowercase();

        assert!(!out.contains("<script"), "script element survived: {out}");
        assert!(!out.contains("onload"), "onload handler survived: {out}");
        assert!(
            !out.contains("javascript:"),
            "javascript: url survived: {out}"
        );
        assert!(
            !out.contains("169.254.169.254"),
            "external reference survived: {out}"
        );
        assert!(!out.contains("alert("), "script payload survived: {out}");
    }

    #[test]
    fn preserves_static_image() {
        let clean = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="red"/></svg>"#;
        let out = sanitize(clean);
        assert!(out.contains("svg"), "svg root dropped: {out}");
        assert!(out.contains("rect"), "rect dropped: {out}");
        assert!(out.contains("red"), "fill color dropped: {out}");
    }
}
