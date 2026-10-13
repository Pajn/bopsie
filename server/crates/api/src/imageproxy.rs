//! `GET /imageproxy/<proxy_id>?size=&fmt=`: artwork by its opaque id, resized. As in Music
//! Assistant it needs no token, sizes come from a fixed set, and `size=0` is the original.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use image::ImageFormat;
use serde::Deserialize;

use crate::Api;

const SIZES: &[u32] = &[0, 80, 160, 256, 512, 1024];
/// Resized images kept in memory. The cache is dropped whole when full.
const CACHE_ENTRIES: usize = 512;

/// An image id, a size and a content type.
type ThumbnailKey = (String, u32, &'static str);

#[derive(Default)]
pub struct Thumbnails {
    cache: Mutex<HashMap<ThumbnailKey, Bytes>>,
}

#[derive(Deserialize)]
pub struct Params {
    #[serde(default)]
    size: Option<u32>,
    #[serde(default)]
    fmt: Option<String>,
}

fn bad_request(reason: &str) -> Response {
    (StatusCode::BAD_REQUEST, reason.to_owned()).into_response()
}

fn image(content_type: &str, bytes: impl Into<Bytes>) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type.to_owned()),
            (header::CACHE_CONTROL, "public, max-age=86400".to_owned()),
        ],
        bytes.into(),
    )
        .into_response()
}

fn format(name: Option<&str>) -> Option<(ImageFormat, &'static str)> {
    match name.unwrap_or("jpeg") {
        "jpeg" | "jpg" => Some((ImageFormat::Jpeg, "image/jpeg")),
        "png" => Some((ImageFormat::Png, "image/png")),
        _ => None,
    }
}

/// Fits the image within `size` pixels square and encodes it.
pub fn resize(bytes: &[u8], size: u32, format: ImageFormat) -> Result<Vec<u8>, image::ImageError> {
    let mut picture = image::load_from_memory(bytes)?;
    if picture.width() > size || picture.height() > size {
        picture = picture.thumbnail(size, size);
    }
    if format == ImageFormat::Jpeg {
        // JPEG has no alpha channel.
        picture = image::DynamicImage::ImageRgb8(picture.to_rgb8());
    }
    let mut out = Vec::new();
    picture.write_to(&mut Cursor::new(&mut out), format)?;
    Ok(out)
}

pub async fn serve(
    State(api): State<Arc<Api>>,
    Path(proxy_id): Path<String>,
    Query(params): Query<Params>,
) -> Response {
    if proxy_id.len() != 64 || !proxy_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return bad_request("Invalid image id");
    }
    let size = params.size.unwrap_or(0);
    if !SIZES.contains(&size) {
        return bad_request("Invalid size");
    }
    let Some((format, content_type)) = format(params.fmt.as_deref()) else {
        return bad_request("Invalid format");
    };

    let key = (proxy_id.clone(), size, content_type);
    if let Some(cached) = api.thumbnails.cache.lock().unwrap().get(&key).cloned() {
        return image(content_type, cached);
    }
    let original = match api.library.image(proxy_id).await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return (StatusCode::NOT_FOUND, "Image not found").into_response(),
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response();
        }
    };
    if size == 0 {
        let original_type = match image::guess_format(&original) {
            Ok(ImageFormat::Png) => "image/png",
            _ => "image/jpeg",
        };
        return image(original_type, original);
    }
    let resized = tokio::task::spawn_blocking(move || resize(&original, size, format)).await;
    let bytes = match resized {
        Ok(Ok(bytes)) => Bytes::from(bytes),
        Ok(Err(error)) => {
            tracing::warn!(%error, "cannot resize an image");
            return (StatusCode::NOT_FOUND, "Image not found").into_response();
        }
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response();
        }
    };
    let mut cache = api.thumbnails.cache.lock().unwrap();
    if cache.len() >= CACHE_ENTRIES {
        cache.clear();
    }
    cache.insert(key, bytes.clone());
    image(content_type, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resizes_within_the_bound_and_keeps_aspect() {
        let picture = image::RgbaImage::from_pixel(400, 200, image::Rgba([10, 20, 30, 128]));
        let mut png = Vec::new();
        picture
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();

        let jpeg = resize(&png, 160, ImageFormat::Jpeg).unwrap();
        let decoded = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (160, 80));
        assert_eq!(image::guess_format(&jpeg).unwrap(), ImageFormat::Jpeg);

        // Smaller images are not enlarged.
        let same = resize(&png, 1024, ImageFormat::Png).unwrap();
        let decoded = image::load_from_memory(&same).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (400, 200));
    }
}
