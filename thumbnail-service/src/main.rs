use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::Error;
use image::ImageFormat;
use poem::http::StatusCode;
#[cfg(not(debug_assertions))]
use poem::listener::{Listener, RustlsCertificate, RustlsConfig};
use poem::listener::TcpListener;
use poem::{Route, Server, get, handler};
use poem::web::{Query, RealIp};
use serde::Deserialize;

const UPSTREAM_BASE: &str = "https://media.scistarter.org";
const CACHE_DIR: &str = "./cache";

#[derive(Debug, Deserialize)]
struct ThumbnailParams {
    w: Option<u32>,
    h: Option<u32>,
}

/// Build the cache file path for a given request path and size.
///
/// The cache layout mirrors the upstream path structure with an added
/// size suffix before the extension. For example, a request for
/// `/images/photo.jpg?w=300` caches as `./cache/images/photo_w300.jpg`.
fn cache_path(request_path: &str, params: &ThumbnailParams) -> PathBuf {
    let clean = request_path.trim_start_matches('/');

    let size_suffix = match (params.w, params.h) {
        (Some(w), Some(h)) => format!("_{}x{}", w, h),
        (Some(w), None) => format!("_w{}", w),
        (None, Some(h)) => format!("_h{}", h),
        (None, None) => String::new(),
    };

    let path = Path::new(clean);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    let filename = if ext.is_empty() {
        format!("{}{}", stem, size_suffix)
    } else {
        format!("{}{}.{}", stem, size_suffix, ext)
    };

    let mut result = PathBuf::from(CACHE_DIR);
    if let Some(parent) = path.parent() {
        result.push(parent);
    }
    result.push(filename);
    result
}

/// Guess the content type from a file extension.
fn content_type_for_ext(ext: &str) -> &str {
    match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "tiff" | "tif" => "image/tiff",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
}

/// Map a file extension to an image::ImageFormat for encoding.
fn image_format_for_ext(ext: &str) -> Option<ImageFormat> {
    match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "png" => Some(ImageFormat::Png),
        "gif" => Some(ImageFormat::Gif),
        "webp" => Some(ImageFormat::WebP),
        "bmp" => Some(ImageFormat::Bmp),
        "tiff" | "tif" => Some(ImageFormat::Tiff),
        "avif" => Some(ImageFormat::Avif),
        _ => None,
    }
}

#[handler]
async fn thumbnail(
    req: &poem::Request,
    Query(params): Query<ThumbnailParams>,
    RealIp(real_ip): RealIp,
) -> poem::Response {
    let request_path = req.uri().path();

    let ext = Path::new(request_path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let content_type = content_type_for_ext(&ext);

    // No resize requested — proxy directly
    if params.w.is_none() && params.h.is_none() {
        return proxy_passthrough(request_path, content_type).await;
    }

    // Check cache first
    let cached = cache_path(request_path, &params);
    if let Ok(data) = tokio::fs::read(&cached).await {
        return poem::Response::builder()
            .status(StatusCode::OK)
            .content_type(content_type)
            .header("X-Cache", "hit")
            .body(data);
    }

    // Download the original
    let upstream_url = format!("{}{}", UPSTREAM_BASE, request_path);
    let bytes = match download(&upstream_url).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to download {}: {}", upstream_url, e);
            return poem::Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(format!("Upstream fetch failed: {}", e));
        }
    };

    // If it's not a raster image format we can process, serve as-is
    let Some(output_format) = image_format_for_ext(&ext) else {
        return poem::Response::builder()
            .status(StatusCode::OK)
            .content_type(content_type)
            .body(bytes);
    };

    // Decode and resize
    let scaled = match resize_image(&bytes, &params, output_format) {
        Ok(data) => data,
        Err(e) => {
            eprintln!(
                "Failed to resize {} ({}): {} — serving original",
                request_path,
                real_ip.map_or("unknown".into(), |ip| ip.to_string()),
                e
            );
            return poem::Response::builder()
                .status(StatusCode::OK)
                .content_type(content_type)
                .body(bytes);
        }
    };

    // Try to cache the result
    if let Err(e) = save_to_cache(&cached, &scaled).await {
        eprintln!("Warning: failed to cache {}: {}", cached.display(), e);
    }

    poem::Response::builder()
        .status(StatusCode::OK)
        .content_type(content_type)
        .header("X-Cache", "miss")
        .body(scaled)
}

async fn proxy_passthrough(request_path: &str, content_type: &str) -> poem::Response {
    let upstream_url = format!("{}{}", UPSTREAM_BASE, request_path);
    match download(&upstream_url).await {
        Ok(bytes) => poem::Response::builder()
            .status(StatusCode::OK)
            .content_type(content_type)
            .body(bytes),
        Err(e) => {
            eprintln!("Failed to download {}: {}", upstream_url, e);
            poem::Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(format!("Upstream fetch failed: {}", e))
        }
    }
}

async fn download(url: &str) -> Result<Vec<u8>, Error> {
    let response = reqwest::get(url).await?;

    if !response.status().is_success() {
        anyhow::bail!("upstream returned {}", response.status());
    }

    Ok(response.bytes().await?.to_vec())
}

fn resize_image(
    data: &[u8],
    params: &ThumbnailParams,
    output_format: ImageFormat,
) -> Result<Vec<u8>, Error> {
    let img = image::load_from_memory(data)?;
    let (orig_w, orig_h) = (img.width(), img.height());

    let (target_w, target_h) = match (params.w, params.h) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => {
            let h = (w as f64 / orig_w as f64 * orig_h as f64).round() as u32;
            (w, h.max(1))
        }
        (None, Some(h)) => {
            let w = (h as f64 / orig_h as f64 * orig_w as f64).round() as u32;
            (w.max(1), h)
        }
        (None, None) => unreachable!(),
    };

    // Don't upscale
    let (target_w, target_h) = if target_w >= orig_w && target_h >= orig_h {
        (orig_w, orig_h)
    } else {
        (target_w, target_h)
    };

    let resized = img.resize(target_w, target_h, image::imageops::FilterType::Lanczos3);

    let mut buf = Cursor::new(Vec::new());
    resized.write_to(&mut buf, output_format)?;
    Ok(buf.into_inner())
}

async fn save_to_cache(path: &Path, data: &[u8]) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    match tokio::fs::write(path, data).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::StorageFull => {
            eprintln!(
                "Warning: not enough disk space to cache {}",
                path.display()
            );
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let default_addr = if cfg!(debug_assertions) {
        "0.0.0.0:3080"
    } else {
        "0.0.0.0:443"
    };
    let listen_on = std::env::var("LISTEN_ON").unwrap_or_else(|_| default_addr.to_string());

    let app = Route::new().at("/*path", get(thumbnail));

    #[cfg(not(debug_assertions))]
    {
        let config = RustlsConfig::new().fallback(
            RustlsCertificate::new()
                .cert(env!("TLS_CERT"))
                .key(env!("TLS_KEY")),
        );

        eprintln!("thumbnail-service listening on {} (TLS)", listen_on);

        Server::new(TcpListener::bind(listen_on).rustls(config))
            .run(app)
            .await?;
    }

    #[cfg(debug_assertions)]
    {
        eprintln!("thumbnail-service listening on {}", listen_on);

        Server::new(TcpListener::bind(listen_on))
            .run(app)
            .await?;
    }

    Ok(())
}
