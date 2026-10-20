//! Re-hosting images that partners point us at.
//!
//! Imported projects name their image by URL. The image is fetched once and
//! kept among our untracked uploads, so the page does not depend on the
//! partner's server staying up, as orrery's `url_to_file` does. The URL comes
//! from outside, so it is fetched through [`crate::ssrf::guarded_get`].

use ulid::Ulid;

use super::{
    STORAGE,
    storage::{FileAccess, FileMetadata},
};

/// Fetch a remote image and re-host it adjacent to our untracked uploads,
/// returning the hosted path (`/-uploaded/<id>`), attributed to
/// `uploaded_by`. Returns None on any fetch/upload failure so the caller can
/// report it.
pub async fn rehost_image_url(url: &str, uploaded_by: Ulid) -> Option<String> {
    let response = crate::ssrf::guarded_get(url).await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let data = response.bytes().await.ok()?.to_vec();
    if data.is_empty() {
        return None;
    }

    let metadata = FileMetadata {
        file_id: Ulid::new(),
        original_filename: rehosted_filename(url, content_type.as_deref()),
        content_type,
        size: data.len() as u64,
        uploaded_by,
        access: FileAccess::Untracked,
        variant: None,
        force_download: false,
    };
    STORAGE.upload_file(metadata, data).await.ok()
}

/// The name a re-hosted image is stored under: the last segment of its URL,
/// as orrery's `url_to_file` names it. The name's extension becomes the stored
/// object's, which is what lets the thumbnail service resize the image, so a
/// name without one takes it from the image's content type.
fn rehosted_filename(url: &str, content_type: Option<&str>) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    let name = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|name| !name.is_empty())
        .unwrap_or("image");
    if std::path::Path::new(name).extension().is_some() {
        return Some(name.to_string());
    }
    let essence = content_type?.split(';').next()?.trim().to_ascii_lowercase();
    let extension = match essence.as_str() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/avif" => "avif",
        _ => return Some(name.to_string()),
    };
    Some(format!("{name}.{extension}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rehosted_images_are_named_after_their_url() {
        assert_eq!(
            rehosted_filename(
                "https://example.org/a/b/photo.JPG?size=big",
                Some("image/jpeg")
            ),
            Some("photo.JPG".to_string())
        );
    }

    #[test]
    fn a_name_without_an_extension_takes_one_from_the_content_type() {
        assert_eq!(
            rehosted_filename("https://cdn.example.org/img/12345", Some("image/png; q=1")),
            Some("12345.png".to_string())
        );
        assert_eq!(
            rehosted_filename("https://cdn.example.org/", Some("image/webp")),
            Some("image.webp".to_string())
        );
        assert_eq!(
            rehosted_filename("https://cdn.example.org/doc", Some("application/pdf")),
            Some("doc".to_string())
        );
    }
}
