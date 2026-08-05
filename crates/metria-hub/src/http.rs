//! HTTP 层：健康检查与静态资源。业务 API 在 S2 阶段加入。

use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};

use crate::assets::lookup;

/// 健康检查响应。
#[derive(Debug, serde::Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
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

pub async fn static_fallback(uri: Uri) -> Response {
    let path = uri.path();
    let static_path = path
        .strip_prefix("/static/")
        .map(|p| p.to_string())
        .unwrap_or_else(|| "index.html".to_string());

    match lookup(&static_path) {
        Some((mime, bytes)) => {
            let mut resp = bytes.into_response();
            resp.headers_mut()
                .insert("content-type", mime.parse().expect("valid mime"));
            resp
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
