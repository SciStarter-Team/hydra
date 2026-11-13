//! Embedded videos, loaded from their host only when the visitor asks.
//!
//! An embedded YouTube player sets YouTube's cookies the moment the page that
//! holds it loads, whether or not the visitor ever presses play, and even the
//! `youtube-nocookie.com` player writes to the browser's storage as soon as it
//! runs. So no page of this site carries a player. In its place is a
//! [`VideoPreview`]: the video's thumbnail, served from this site, and a play
//! control. Pressing it is the visitor asking for the video, and only then is
//! the player put into the page. Until that moment the browser has not
//! contacted YouTube at all.
//!
//! The player that is put in is the `youtube-nocookie.com` one, in a
//! `credentialless` frame where the browser supports that (Chromium): such a
//! frame is sent no cookies and is given storage that is thrown away with the
//! page, so even a played video leaves nothing behind there.
//!
//! # Where previews come from
//!
//! Templates that show a video render a [`VideoPreview`] themselves. Content
//! written by staff --- page-builder pages and components, HTML pages,
//! embedded WordPress pages --- is written with ordinary `<iframe>` embeds,
//! and [`VideoPreviews`] rewrites every YouTube iframe in the HTML the public
//! site serves into a preview on its way out. That is what makes the
//! guarantee hold for content nobody has reviewed: an editor can paste
//! YouTube's own embed code and the page still loads nothing from YouTube.
//! The static HTML files under `/-static` pass through it as well.
//!
//! The rewrite leaves alone the iframes inside a `<template>`, which are
//! inert until a script instantiates them (the preview's own player is one),
//! and those inside an `<ss-rich-text-html>` editor, whose contents are the
//! stored source being edited and must be saved back as they were written.

use anyhow::Error;
use lol_html::html_content::ContentType;
use lol_html::{RewriteStrSettings, element, rewrite_str};
use poem::http::StatusCode;
use poem::web::Path;
use poem::{
    Endpoint, EndpointExt, IntoResponse, Middleware, Request, Response, Route, get, handler,
};
use sailfish::TemplateOnce;

/// A YouTube video, by its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YouTubeVideo {
    /// Drawn only from YouTube's id alphabet, so it is safe in any URL.
    id: String,
}

impl YouTubeVideo {
    /// The video a YouTube watch, share, shorts, live or embed URL refers
    /// to, or `None` if `url` is not one. A scheme-relative URL (`//www…`),
    /// as embed snippets are sometimes written, is read as HTTPS.
    pub fn from_url(url: &str) -> Option<YouTubeVideo> {
        let url = url.trim();
        let parsed = match url.strip_prefix("//") {
            Some(rest) => url::Url::parse(&format!("https://{rest}")),
            None => url::Url::parse(url),
        }
        .ok()?;

        if !matches!(parsed.scheme(), "http" | "https") {
            return None;
        }

        let host = parsed
            .host_str()?
            .trim_start_matches("www.")
            .trim_start_matches("m.");
        let mut segments = parsed.path_segments()?.filter(|s| !s.is_empty());

        let id = match host {
            "youtu.be" => segments.next()?.to_string(),
            "youtube.com" | "youtube-nocookie.com" => match segments.next()? {
                "watch" => parsed
                    .query_pairs()
                    .find(|(key, _)| key == "v")
                    .map(|(_, value)| value.into_owned())?,
                "embed" | "shorts" | "live" | "v" => segments.next()?.to_string(),
                _ => return None,
            },
            _ => return None,
        };

        YouTubeVideo::from_id(&id)
    }

    /// The video with this id, or `None` if `id` is not a well-formed one.
    pub fn from_id(id: &str) -> Option<YouTubeVideo> {
        // Video ids are drawn from this alphabet; anything else is not one,
        // and must not reach a URL built from it.
        let well_formed = !id.is_empty()
            && id.len() <= 64
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

        well_formed.then(|| YouTubeVideo { id: id.to_string() })
    }

    /// The video's page on YouTube, for a visitor without scripts.
    pub fn watch_url(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.id)
    }

    /// The player put into the page when the visitor asks for the video.
    /// It starts at once, since asking for it was pressing play.
    pub fn player_url(&self) -> String {
        format!(
            "https://www.youtube-nocookie.com/embed/{}?autoplay=1",
            self.id
        )
    }

    /// The video's thumbnail, as this site serves it.
    pub fn thumbnail_path(&self) -> String {
        format!("/-video/youtube/{}/thumbnail.jpg", self.id)
    }

    /// The thumbnail at YouTube, which only this server ever fetches.
    fn upstream_thumbnail_url(&self) -> String {
        format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", self.id)
    }
}

/// A video's thumbnail and play control, standing in for its player until
/// the visitor asks for it.
///
/// Styled entirely inline: a preview may be rewritten into an HTML page that
/// loads none of this site's CSS, or into embedded WordPress content, where
/// a cascade layer reverts every rule of ours.
#[derive(TemplateOnce)]
#[template(path = "component/video_preview.stpl.html")]
pub struct VideoPreview<'a> {
    pub video: &'a YouTubeVideo,
    /// What the video is, for the play control's label and the player's
    /// title.
    pub title: &'a str,
    /// Classes for the preview's box.
    pub class: &'a str,
    /// Inline style for the preview's box. Must give it a size and a
    /// position, since the thumbnail and the player fill it absolutely;
    /// [`preview_box_style`] works one out from an iframe's own.
    pub style: &'a str,
}

/// The inline style for a preview that takes the place of an iframe with the
/// given `class`, `style` and `width`/`height` attributes, so that it occupies
/// the same box.
///
/// An iframe has an intrinsic size and the `<div>` replacing it does not, so
/// a box the iframe's markup does not size is given the aspect ratio its
/// `width` and `height` attributes describe, or 16:9 without them. And the
/// box is made a positioned element, unless it already is one, so that the
/// thumbnail and the player can fill it.
pub fn preview_box_style(
    class: &str,
    style: &str,
    width: Option<&str>,
    height: Option<&str>,
) -> String {
    let declarations = style.to_ascii_lowercase().replace(char::is_whitespace, "");
    let utilities: Vec<&str> = class
        .split_whitespace()
        .map(|token| token.rsplit(':').next().unwrap_or(token))
        .collect();

    let positioned = declarations.contains("position:")
        || utilities
            .iter()
            .any(|u| matches!(*u, "absolute" | "fixed" | "relative" | "sticky"));
    let sized = declarations.contains("height:")
        || declarations.contains("aspect-ratio:")
        || utilities
            .iter()
            .any(|u| u.starts_with("h-") || u.starts_with("aspect-") || u.starts_with("size-"));

    let mut computed = String::new();

    if !sized {
        let dimension = |value: Option<&str>| {
            value
                .and_then(|v| v.trim().trim_end_matches("px").parse::<u32>().ok())
                .filter(|v| *v > 0)
        };

        match (dimension(width), dimension(height)) {
            (Some(width), Some(height)) => computed.push_str(&format!(
                "width:100%;max-width:{width}px;aspect-ratio:{width} / {height};"
            )),
            _ => computed.push_str("width:100%;aspect-ratio:16 / 9;"),
        }
    }

    if !positioned {
        computed.push_str("position:relative;");
    }

    // The iframe's own declarations come last, so that where they say
    // something about the box they are the ones that hold.
    computed.push_str(style);
    computed
}

/// Rewrites every YouTube iframe in `html` into a [`VideoPreview`]. Returns
/// `None` when there was nothing to rewrite, which is the usual case and
/// costs only a scan for `<iframe`.
pub fn rewrite_embeds(html: &str) -> Result<Option<String>, Error> {
    const MARKER: &[u8] = b"<iframe";

    let has_iframe = html
        .as_bytes()
        .windows(MARKER.len())
        .any(|window| window.eq_ignore_ascii_case(MARKER));

    if !has_iframe {
        return Ok(None);
    }

    // Handlers run in the order given, so the first two mark the iframes
    // that are to be kept before the last one decides.
    const KEEP: &str = "data-video-preview-keep";

    let rewritten = rewrite_str(
        html,
        RewriteStrSettings::new()
            .append_element_content_handler(element!("template iframe", |el| {
                el.set_attribute(KEEP, "")?;
                Ok(())
            }))
            .append_element_content_handler(element!("ss-rich-text-html iframe", |el| {
                el.set_attribute(KEEP, "")?;
                Ok(())
            }))
            .append_element_content_handler(element!("iframe[src]", |el| {
                if el.has_attribute(KEEP) {
                    el.remove_attribute(KEEP);
                    return Ok(());
                }

                let Some(video) = el
                    .get_attribute("src")
                    .and_then(|src| YouTubeVideo::from_url(&src))
                else {
                    return Ok(());
                };

                let class = el.get_attribute("class").unwrap_or_default();
                let style = preview_box_style(
                    &class,
                    &el.get_attribute("style").unwrap_or_default(),
                    el.get_attribute("width").as_deref(),
                    el.get_attribute("height").as_deref(),
                );
                let title = el
                    .get_attribute("title")
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| crate::i18n::tr("Video"));

                let preview = VideoPreview {
                    video: &video,
                    title: &title,
                    class: &class,
                    style: &style,
                }
                .render_once()?;

                el.replace(&preview, ContentType::Html);
                Ok(())
            })),
    )?;

    Ok(Some(rewritten))
}

/// Applies [`rewrite_embeds`] to every HTML response.
pub struct VideoPreviews;

pub struct VideoPreviewsEndpoint<E> {
    wrapped: E,
}

impl<E: Endpoint> Middleware<E> for VideoPreviews {
    type Output = VideoPreviewsEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        VideoPreviewsEndpoint { wrapped: ep }
    }
}

impl<E: Endpoint> Endpoint for VideoPreviewsEndpoint<E> {
    type Output = Response;

    async fn call(&self, req: Request) -> poem::Result<Self::Output> {
        let mut response = self.wrapped.call(req).await?.into_response();

        let is_html = response
            .content_type()
            .is_some_and(|content_type| content_type.starts_with("text/html"));

        if !is_html {
            return Ok(response);
        }

        let body = response.take_body().into_string().await?;

        match rewrite_embeds(&body) {
            Ok(Some(rewritten)) => response.set_body(rewritten),
            Ok(None) => response.set_body(body),
            Err(error) => {
                // Serving the page with its players in place would load them
                // on every visit, which is the very thing this exists to
                // prevent; better a failed page than a quietly leaky one.
                tracing::error!("Rewriting video embeds failed: {error:#}");
                return Err(poem::Error::from_status(StatusCode::INTERNAL_SERVER_ERROR));
            }
        }

        Ok(response)
    }
}

/// A video's thumbnail, served from this site; see [`crate::remote_image`].
#[handler]
async fn youtube_thumbnail(Path(id): Path<String>) -> poem::Result<Response> {
    let video = YouTubeVideo::from_id(&id).ok_or(StatusCode::NOT_FOUND)?;

    crate::remote_image::serve(&video.upstream_thumbnail_url()).await
}

/// Routes under `/-video`. Mounted outside the session middleware: a
/// thumbnail needs no session, and must not start one.
pub fn routes() -> impl Endpoint {
    Route::new().at("/youtube/:id/thumbnail.jpg", get(youtube_thumbnail).boxed())
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    #[test]
    fn youtube_urls_name_their_video() {
        let video = YouTubeVideo::from_id("dQw4w9WgXcQ");

        for url in [
            "https://youtu.be/dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ?t=42",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=x",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
            "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?rel=0",
            "//www.youtube.com/embed/dQw4w9WgXcQ",
        ] {
            check!(YouTubeVideo::from_url(url) == video, "{url}");
        }
    }

    #[test]
    fn other_urls_are_not_youtube_videos() {
        for url in [
            "https://vimeo.com/123",
            "https://www.youtube.com/channel/abc",
            "https://youtu.be/",
            "javascript:alert(1)",
            "https://youtu.be/abc%22onload",
            "/embed/dQw4w9WgXcQ",
            "not a url",
        ] {
            check!(YouTubeVideo::from_url(url) == None, "{url}");
        }
    }

    #[test]
    fn an_unsized_iframe_gets_a_sixteen_by_nine_positioned_box() {
        check!(
            preview_box_style("w-full", "", None, None)
                == "width:100%;aspect-ratio:16 / 9;position:relative;"
        );
    }

    #[test]
    fn width_and_height_attributes_give_the_box_its_proportions() {
        check!(
            preview_box_style("", "", Some("560"), Some("315"))
                == "width:100%;max-width:560px;aspect-ratio:560 / 315;position:relative;"
        );
    }

    #[test]
    fn a_box_the_markup_sizes_and_positions_is_left_to_it() {
        let style = "position:absolute;top:0;left:0;width:100%;height:100%;border:0;";
        check!(preview_box_style("", style, None, None) == style);

        check!(preview_box_style("w-full aspect-video", "", None, None) == "position:relative;");
        check!(
            preview_box_style("w-full", "aspect-ratio: 16 / 9;", None, None)
                == "position:relative;aspect-ratio: 16 / 9;"
        );
    }

    #[test]
    fn pages_without_iframes_are_not_rewritten() {
        check!(rewrite_embeds("<p>No video here</p>").unwrap() == None);
    }

    #[test]
    fn youtube_iframes_become_previews_and_others_are_kept() {
        let html = r#"<div><iframe src="https://www.youtube.com/embed/dQw4w9WgXcQ" title="Clip" class="w-full aspect-video"></iframe><iframe src="https://example.org/map"></iframe></div>"#;

        let rewritten = rewrite_embeds(html).unwrap().unwrap();

        check!(!rewritten.contains(r#"<iframe src="https://www.youtube.com"#));
        check!(rewritten.contains("data-video-preview"));
        check!(rewritten.contains("/-video/youtube/dQw4w9WgXcQ/thumbnail.jpg"));
        check!(rewritten.contains("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
        check!(rewritten.contains(r#"<iframe src="https://example.org/map"></iframe>"#));
    }

    #[test]
    fn the_preview_player_survives_a_second_rewrite() {
        let video = YouTubeVideo::from_id("dQw4w9WgXcQ").unwrap();
        let preview = VideoPreview {
            video: &video,
            title: "Clip",
            class: "",
            style: "position:relative;",
        }
        .render_once()
        .unwrap();

        // Marking the player to keep it re-serializes its start tag, so the
        // markup is not byte-identical; what matters is that the player is
        // still there and was not itself turned into a preview.
        let rewritten = rewrite_embeds(&preview).unwrap().unwrap();

        check!(rewritten.matches("data-video-preview").count() == 2);
        check!(rewritten.contains(&video.player_url()));
        check!(!rewritten.contains("data-video-preview-keep"));
    }

    #[test]
    fn iframes_in_an_html_editor_are_kept() {
        let html = r#"<ss-rich-text-html name="body"><iframe src="https://www.youtube.com/embed/dQw4w9WgXcQ"></iframe></ss-rich-text-html>"#;

        check!(rewrite_embeds(html).unwrap().unwrap() == html);
    }
}
