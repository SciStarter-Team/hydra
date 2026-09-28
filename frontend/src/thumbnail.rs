//! Resized copies of images, from the thumbnail service
//! (`thumbnail-service/`, served at [`THUMBNAIL_HOST`]).
//!
//! The service answers any path it can fetch from `media.scistarter.org`,
//! resized to fit the `w`×`h` box without upscaling. So an image can be
//! thumbnailed only if it lives there under a path with an image extension;
//! anything else is left as it is.

const THUMBNAIL_HOST: &str = "https://thumbnail.scistarter.org";

/// Hosts serving the files the thumbnail service reads. `media.scistarter.org`
/// fronts the `orrery-media` bucket, which is where orrery's uploads (profile
/// photos among them) are linked from directly.
const SOURCE_HOSTS: &[&str] = &["media.scistarter.org", "orrery-media.s3.amazonaws.com"];

/// The extensions the service will resize.
const EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "avif",
];

/// `image` resized to fit `width`×`height`, or `image` unchanged when the
/// thumbnail service cannot serve it.
///
/// Ask for twice the displayed size, so the result stays sharp on high
/// density screens.
pub fn thumbnail_url(image: &str, width: u32, height: u32) -> String {
    let Ok(url) = url::Url::parse(image) else {
        return image.to_string();
    };
    let servable = matches!(url.scheme(), "http" | "https")
        && url
            .host_str()
            .is_some_and(|host| SOURCE_HOSTS.contains(&host))
        && url
            .path()
            .rsplit_once('.')
            .is_some_and(|(_, ext)| EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()));
    if servable {
        format!("{THUMBNAIL_HOST}{}?w={width}&h={height}", url.path())
    } else {
        image.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orrery_media_is_thumbnailed() {
        assert_eq!(
            thumbnail_url(
                "https://orrery-media.s3.amazonaws.com/user/2019-05/DE585DD4.jpeg",
                150,
                150
            ),
            "https://thumbnail.scistarter.org/user/2019-05/DE585DD4.jpeg?w=150&h=150"
        );
        assert_eq!(
            thumbnail_url("https://media.scistarter.org/user/a%20b.PNG?x=1", 64, 64),
            "https://thumbnail.scistarter.org/user/a%20b.PNG?w=64&h=64"
        );
    }

    #[test]
    fn anything_else_is_left_alone() {
        for image in [
            "/-uploaded/01JKXM2YPAW1HXSRAHSJPY9ARW",
            "https://example.org/me.jpg",
            "https://media.scistarter.org/foundations/index.html",
            "https://media.scistarter.org/user/no-extension",
            "not a url",
        ] {
            assert_eq!(thumbnail_url(image, 150, 150), image);
        }
    }
}
