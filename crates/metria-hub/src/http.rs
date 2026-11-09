//! HTTP 层：健康检查与静态资源。业务 API 在 S2 阶段加入。

use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use sha2::{Digest, Sha256};
use tower_http::compression::predicate::{NotForContentType, Predicate, SizeAbove};
use tower_http::compression::CompressionLayer;

use crate::assets::{lookup, StaticAsset};

/// 健康检查响应。
#[derive(Debug, serde::Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

/// 文本类静态/API 响应压缩；排除 SSE、图片、字体与二进制。
fn compression_predicate() -> impl Predicate {
    SizeAbove::new(1024)
        .and(NotForContentType::SSE)
        .and(NotForContentType::IMAGES)
        .and(NotForContentType::const_new("font/"))
        .and(NotForContentType::const_new("application/octet-stream"))
}

/// 给完整应用路由套上压缩层（SSE 由 predicate 排除）。
pub fn with_compression(router: Router) -> Router {
    router.layer(CompressionLayer::new().compress_when(compression_predicate()))
}

/// 构建应用路由（S0：healthz + 静态资源 fallback）。
pub fn app_router() -> Router {
    Router::new()
        .route("/healthz", get(health))
        .fallback(static_fallback)
}

async fn health() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok",
        version: crate::VERSION,
    })
}

pub async fn static_fallback(method: Method, headers: HeaderMap, uri: Uri) -> Response {
    let path = uri.path();
    let static_path = path
        .strip_prefix("/static/")
        .map(|p| p.to_string())
        .unwrap_or_else(|| "index.html".to_string());

    match lookup(&static_path) {
        Some(asset) => static_response(method, &headers, asset),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn static_response(method: Method, headers: &HeaderMap, asset: StaticAsset) -> Response {
    let etag = etag_for(&asset);
    let cache_control = cache_control(&asset);
    let mime = asset.mime;

    if if_none_match(headers, &etag) {
        let mut resp = StatusCode::NOT_MODIFIED.into_response();
        apply_headers(&mut resp, mime, &etag, cache_control);
        return resp;
    }

    if method == Method::HEAD {
        let mut resp = Response::new(Body::empty());
        apply_headers(&mut resp, mime, &etag, cache_control);
        resp.headers_mut().insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&asset.bytes.len().to_string()).expect("content length"),
        );
        return resp;
    }

    let mut resp = asset.bytes.into_response();
    apply_headers(&mut resp, mime, &etag, cache_control);
    resp
}

fn apply_headers(resp: &mut Response, mime: &str, etag: &str, cache_control: &str) {
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime).expect("valid mime"),
    );
    h.insert(
        header::ETAG,
        HeaderValue::from_str(etag).expect("valid etag"),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_str(cache_control).expect("valid cache control"),
    );
}

fn cache_control(asset: &StaticAsset) -> &'static str {
    if asset.is_index || asset.path == "index.html" {
        "no-cache"
    } else if asset.path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=86400"
    }
}

fn etag_for(asset: &StaticAsset) -> String {
    if asset.path.starts_with("assets/") {
        if let Some(name) = asset.path.rsplit('/').next() {
            if let Some((stem, _ext)) = name.rsplit_once('.') {
                if let Some((_prefix, hash)) = stem.rsplit_once('-') {
                    if !hash.is_empty() {
                        return format!("\"{hash}\"");
                    }
                }
            }
        }
    }
    format!("\"{:x}\"", Sha256::digest(&asset.bytes))
}

fn if_none_match(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .any(|candidate| candidate.trim().trim_start_matches("W/") == etag)
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_policy_matches_asset_type() {
        let asset = |path: &str, is_index: bool| StaticAsset {
            mime: "text/plain",
            bytes: Vec::new(),
            path: path.to_string(),
            is_index,
        };
        assert_eq!(
            cache_control(&asset("assets/index-abc.js", false)),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(cache_control(&asset("index.html", true)), "no-cache");
        assert_eq!(
            cache_control(&asset("fonts/MapleMono-Medium.woff2", false)),
            "public, max-age=86400"
        );
    }

    #[test]
    fn compression_predicate_excludes_sse_fonts_and_images() {
        let predicate = compression_predicate();
        let response = |content_type: &str| {
            Response::builder()
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from("x".repeat(2048)))
                .unwrap()
        };
        assert!(predicate.should_compress(&response("text/html; charset=utf-8")));
        assert!(predicate.should_compress(&response("application/javascript")));
        assert!(predicate.should_compress(&response("image/svg+xml")));
        assert!(!predicate.should_compress(&response("text/event-stream")));
        assert!(!predicate.should_compress(&response("font/woff2")));
        assert!(!predicate.should_compress(&response("image/png")));
    }

    #[test]
    fn etag_changes_with_content_for_non_hashed_assets() {
        let asset = |bytes: &[u8]| StaticAsset {
            mime: "text/html",
            bytes: bytes.to_vec(),
            path: "index.html".to_string(),
            is_index: true,
        };
        assert_ne!(etag_for(&asset(b"one")), etag_for(&asset(b"two")));
    }

    #[test]
    fn if_none_match_accepts_weak_and_list_values() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::IF_NONE_MATCH,
            HeaderValue::from_static("W/\"abc\", \"def\""),
        );
        assert!(if_none_match(&headers, "\"abc\""));
        assert!(if_none_match(&headers, "\"def\""));
        assert!(!if_none_match(&headers, "\"other\""));
    }
}
