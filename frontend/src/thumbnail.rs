//! Resized copies of images, from the thumbnail service
//! (`thumbnail-service/`, served at [`THUMBNAIL_HOST`]).
//!
//! The service answers a path by fetching it from our uploads bucket, or
//! failing that from `media.scistarter.org`, and resizing it to fit the `w`×`h`
//! box without upscaling. It needs the path to end in an image extension. So
//! it can thumbnail two kinds of image:
//!
//! - orrery's uploads, linked from `media.scistarter.org` or the
//!   `orrery-media` bucket behind it, whose URL path is the path it needs;
//! - our own untracked uploads, which are linked as `/-uploaded/<id>` but
//!   stored under [`FileMetadata::object_key`], so the path comes from the
//!   file's metadata.
//!
//! Anything else is left as it is. Tracked uploads are never thumbnailed:
//! they are served only to their owner, and a thumbnail link would not be.

use crate::peering::LoadStorePeered;
use crate::upload::storage::{FileAccess, FileMetadata, untracked_file_id};

const THUMBNAIL_HOST: &str = "https://thumbnail.scistarter.org";

/// Hosts serving the files the thumbnail service reads from
/// `media.scistarter.org`, which fronts the `orrery-media` bucket.
const MEDIA_HOSTS: &[&str] = &["media.scistarter.org", "orrery-media.s3.amazonaws.com"];

/// The extensions the service will resize.
const EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "avif",
];

/// `image` resized to fit `width`×`height`, or `image` unchanged when the
/// thumbnail service cannot serve it.
///
/// Ask for twice the displayed size, so the result stays sharp on high
/// density screens.
pub async fn thumbnail_url(image: &str, width: u32, height: u32) -> String {
    let path = match untracked_file_id(image) {
        Some(file_id) => FileMetadata::load(file_id)
            .await
            .ok()
            .and_then(|metadata| upload_path(&metadata)),
        None => media_path(image),
    };
    match path {
        Some(path) => format!("{THUMBNAIL_HOST}/{path}?w={width}&h={height}"),
        None => image.to_string(),
    }
}

/// The service path for one of our uploads, if it is a public image.
fn upload_path(metadata: &FileMetadata) -> Option<String> {
    if !matches!(metadata.access, FileAccess::Untracked) {
        return None;
    }
    let key = metadata.object_key();
    has_image_extension(&key).then_some(key)
}

/// The service path for an orrery image, if `image` is one.
fn media_path(image: &str) -> Option<String> {
    let url = url::Url::parse(image).ok()?;
    let servable = matches!(url.scheme(), "http" | "https")
        && url
            .host_str()
            .is_some_and(|host| MEDIA_HOSTS.contains(&host))
        && has_image_extension(url.path());
    servable.then(|| url.path().trim_start_matches('/').to_string())
}

fn has_image_extension(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ulid::Ulid;

    fn upload(name: Option<&str>, access: FileAccess) -> FileMetadata {
        FileMetadata {
            file_id: "01M3AC977GY2W0R6FF0SHAWXFE".parse().unwrap(),
            original_filename: name.map(str::to_string),
            content_type: None,
            size: 0,
            uploaded_by: Ulid::nil(),
            access,
            variant: None,
            force_download: false,
        }
    }

    #[test]
    fn orrery_media_is_thumbnailed_by_its_path() {
        assert_eq!(
            media_path("https://orrery-media.s3.amazonaws.com/user/2019-05/DE585DD4.jpeg"),
            Some("user/2019-05/DE585DD4.jpeg".to_string())
        );
        assert_eq!(
            media_path("https://media.scistarter.org/user/a%20b.PNG?x=1"),
            Some("user/a%20b.PNG".to_string())
        );
    }

    #[test]
    fn other_urls_are_not_media() {
        for image in [
            "https://example.org/me.jpg",
            "https://media.scistarter.org/foundations/index.html",
            "https://media.scistarter.org/user/no-extension",
            "/-uploaded/01M3AC977GY2W0R6FF0SHAWXFE",
            "not a url",
        ] {
            assert_eq!(media_path(image), None, "{image}");
        }
    }

    #[test]
    fn public_image_uploads_are_thumbnailed_by_their_object_key() {
        assert_eq!(
            upload_path(&upload(Some("IMG 2026.jpg"), FileAccess::Untracked)),
            Some("IMG_2026/01M3AC977GY2W0R6FF0SHAWXFE.jpg".to_string())
        );
    }

    #[test]
    fn private_or_non_image_uploads_are_not() {
        assert_eq!(
            upload_path(&upload(Some("a.jpg"), FileAccess::Tracked)),
            None
        );
        assert_eq!(
            upload_path(&upload(Some("a.pdf"), FileAccess::Untracked)),
            None
        );
        assert_eq!(upload_path(&upload(None, FileAccess::Untracked)), None);
    }

    #[test]
    fn untracked_paths_name_their_file() {
        assert_eq!(
            untracked_file_id("/-uploaded/01M3AC977GY2W0R6FF0SHAWXFE"),
            "01M3AC977GY2W0R6FF0SHAWXFE".parse().ok()
        );
        assert_eq!(
            untracked_file_id("/-uploadedx/01M3AC977GY2W0R6FF0SHAWXFE"),
            None
        );
        assert_eq!(untracked_file_id("/-uploaded/not-a-ulid"), None);
    }
}
