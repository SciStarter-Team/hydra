//! Images held elsewhere, served from this site.
//!
//! A page that shows a picture straight from another company's servers tells
//! that company who looked at the page, and lets it set cookies on the
//! visitor while it is at it. Where a page needs such a picture --- a video's
//! thumbnail, a social media post's photo --- its route fetches the picture
//! itself through [`serve`] and hands it over, so the visitor's browser only
//! ever talks to this site.
//!
//! [`serve`] fetches only the URLs its callers give it, and every caller
//! builds or checks the URL itself (a video id, a host on an allowlist), so
//! no request can make this server fetch an address of the requester's
//! choosing.

use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use poem::Response;
use poem::http::{StatusCode, header};

use crate::lru_cache::LruCache;

/// How long a fetched image is kept, here and by the browser.
const LIFETIME: Duration = Duration::from_secs(60 * 60 * 24);

/// The largest image that is served. Thumbnails and post photos are tens to
/// hundreds of kilobytes; anything far beyond that is not what was meant.
const MAX_BYTES: usize = 5 * 1024 * 1024;

#[derive(Clone)]
struct Image {
    content_type: String,
    bytes: Arc<Vec<u8>>,
}

/// Images already fetched, by URL.
static CACHE: LazyLock<Mutex<LruCache<Image>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(1000, LIFETIME)));

/// Serves the image at `url`, fetched from there the first time it is asked
/// for and from memory after that. The caller is responsible for `url` being
/// one it means this server to fetch.
pub async fn serve(url: &str) -> poem::Result<Response> {
    let cached = CACHE.lock().expect("image cache lock poisoned").get(url);

    let image = match cached {
        Some(image) => image,
        None => {
            let image = fetch(url).await?;

            CACHE
                .lock()
                .expect("image cache lock poisoned")
                .insert(url.to_string(), image.clone());

            image
        }
    };

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, image.content_type)
        .header(
            header::CACHE_CONTROL,
            format!("public, max-age={}", LIFETIME.as_secs()),
        )
        .body(image.bytes.as_ref().clone()))
}

async fn fetch(url: &str) -> poem::Result<Image> {
    let response = reqwest::get(url).await.map_err(|error| {
        tracing::warn!("Fetching the image at {url} failed: {error}");
        StatusCode::BAD_GATEWAY
    })?;

    if !response.status().is_success() {
        return Err(StatusCode::NOT_FOUND.into());
    }

    // Only an image is passed on: whatever else a host might answer with is
    // not something this site should be serving under its own name.
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.starts_with("image/") && !value.starts_with("image/svg"))
        .ok_or(StatusCode::BAD_GATEWAY)?
        .to_string();

    if response
        .content_length()
        .is_some_and(|length| length as usize > MAX_BYTES)
    {
        return Err(StatusCode::BAD_GATEWAY.into());
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    if bytes.len() > MAX_BYTES {
        return Err(StatusCode::BAD_GATEWAY.into());
    }

    Ok(Image {
        content_type,
        bytes: Arc::new(bytes.to_vec()),
    })
}
