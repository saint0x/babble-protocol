//! Object-bound, integrity-checked binary media delivery. Executable resources
//! remain on the separate Surface transport.
use crate::{
    ApiState,
    error::ApiError,
    routes::{hash_value, lock_node, object_id},
};
use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::Response,
};
use babel_judgment::JudgmentProvider;
use std::ops::RangeInclusive;

pub(crate) async fn get<P: JudgmentProvider>(
    State(state): State<ApiState<P>>,
    Path((id, hash)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let id = object_id(id)?;
    let hash = hash_value(hash)?;
    let (media_type, bytes) = {
        let node = lock_node(&state)?;
        let object = node
            .object(&id)
            .ok_or_else(|| ApiError::not_found("object"))?;
        let uri = format!("babel://blobs/{hash}");
        let mut resources = object
            .resources
            .iter()
            .filter(|resource| resource.integrity == hash && resource.uri == uri);
        let resource = resources
            .next()
            .ok_or_else(|| ApiError::not_found("object media"))?;
        if resources.any(|other| !other.media_type.eq_ignore_ascii_case(&resource.media_type)) {
            return Err(ApiError::conflict("ambiguous object media type"));
        }
        let media_type = resource.media_type.to_ascii_lowercase();
        if !supported_type(&media_type) {
            return Err(ApiError::bad_request("unsupported inline media type"));
        }
        let (_, bytes) = node
            .media_blob_bounded(
                &hash,
                &media_type,
                crate::execution::MAX_BUFFERED_BLOB_BYTES,
            )?
            .ok_or_else(|| ApiError::not_found("media blob"))?;
        (media_type, Bytes::from(bytes))
    };
    let length = bytes.len();
    let etag = format!("\"{hash}\"");
    // Range is defined for GET only. Unknown dates and weak/mismatched validators
    // cannot prove identity, so If-Range falls back to the full representation.
    let range = if method == Method::GET && if_range_matches(&headers, &etag) {
        requested_range(&headers, length)
    } else {
        Ok(None)
    };
    let mut response = Response::builder()
        .header(header::CONTENT_TYPE, media_type)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::ETAG, etag)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; sandbox",
        );
    let body = match range {
        Err(()) => {
            response = response
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{length}"))
                .header(header::CONTENT_LENGTH, 0);
            Body::empty()
        }
        Ok(Some(range)) => {
            response = response
                .status(StatusCode::PARTIAL_CONTENT)
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes {}-{}/{length}", range.start(), range.end()),
                )
                .header(header::CONTENT_LENGTH, range.end() - range.start() + 1);
            Body::from(bytes.slice(range))
        }
        Ok(None) => {
            response = response.header(header::CONTENT_LENGTH, length);
            if method == Method::HEAD {
                Body::empty()
            } else {
                Body::from(bytes)
            }
        }
    };
    response
        .body(body)
        .map_err(|_| ApiError::internal("media response encoding failed"))
}

fn supported_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "image/jpeg"
            | "image/png"
            | "image/gif"
            | "image/webp"
            | "image/avif"
            | "image/bmp"
            | "image/x-icon"
            | "image/vnd.microsoft.icon"
            | "audio/mpeg"
            | "audio/mp4"
            | "audio/aac"
            | "audio/ogg"
            | "audio/wav"
            | "audio/x-wav"
            | "audio/webm"
            | "audio/flac"
            | "video/mp4"
            | "video/webm"
            | "video/ogg"
            | "video/quicktime"
    )
}

fn if_range_matches(headers: &HeaderMap, etag: &str) -> bool {
    let mut values = headers.get_all(header::IF_RANGE).iter();
    match values.next() {
        None => true,
        Some(value) => {
            values.next().is_none() && value.to_str().is_ok_and(|value| value.trim() == etag)
        }
    }
}

fn requested_range(
    headers: &HeaderMap,
    length: usize,
) -> Result<Option<RangeInclusive<usize>>, ()> {
    let mut values = headers.get_all(header::RANGE).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(());
    }
    let value = value.to_str().map_err(|_| ())?.trim();
    let (unit, interval) = value.split_once('=').ok_or(())?;
    if !unit.eq_ignore_ascii_case("bytes") || length == 0 {
        return Err(());
    }
    let (start, end) = interval.split_once('-').ok_or(())?;
    if start.is_empty() {
        let suffix = decimal(end)?;
        if suffix == 0 {
            return Err(());
        }
        return Ok(Some(length.saturating_sub(suffix)..=length - 1));
    }
    let start = decimal(start)?;
    let end = if end.is_empty() {
        length - 1
    } else {
        decimal(end)?
    };
    if start >= length || end < start {
        return Err(());
    }
    Ok(Some(start..=end.min(length - 1)))
}

// Saturation handles arbitrarily large valid decimal suffix/end values without
// wrapping. Starts beyond the representation still fail the bounds check.
fn decimal(value: &str) -> Result<usize, ()> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    Ok(value.bytes().fold(0usize, |number, digit| {
        number
            .saturating_mul(10)
            .saturating_add(usize::from(digit - b'0'))
    }))
}

#[cfg(test)]
#[path = "tests/media.rs"]
mod tests;
