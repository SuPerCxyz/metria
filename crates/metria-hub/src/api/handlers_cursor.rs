//! Collector 游标同步 API（无状态轮询采集）。

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use metria_protocol::{validate_cursor_sync, CursorEntry, CursorSyncRequest, CursorSyncResponse};

use super::{json_err, CollectorIdentity};
use crate::AppState;

fn missing_identity() -> Response {
    json_err(
        StatusCode::BAD_REQUEST,
        "collector_identity_missing",
        "collector 未注册（env bootstrap token 无身份，无法同步游标）",
    )
}

/// GET /api/v1/collectors/cursors：返回当前 collector 全部 Source 游标。
pub(crate) async fn cursors_get(
    State(st): State<AppState>,
    identity: Option<Extension<CollectorIdentity>>,
) -> Response {
    let Some(Extension(id)) = identity else {
        return missing_identity();
    };
    let cursors = st
        .db
        .list_source_cursors(&id.collector_id)
        .into_iter()
        .map(|(source_id, cursor_json, updated_at)| CursorEntry {
            source_id,
            cursor_json,
            updated_at: Some(updated_at),
        })
        .collect();
    Json(CursorSyncResponse { cursors }).into_response()
}

/// POST /api/v1/collectors/cursors：事件确认上传后推进游标（幂等 upsert）。
pub(crate) async fn cursors_push(
    State(st): State<AppState>,
    identity: Option<Extension<CollectorIdentity>>,
    Json(req): Json<CursorSyncRequest>,
) -> Response {
    let Some(Extension(id)) = identity else {
        return missing_identity();
    };
    if let Err(e) = validate_cursor_sync(&req) {
        return json_err(StatusCode::BAD_REQUEST, "invalid_cursor_sync", &e);
    }
    let items: Vec<(String, String)> = req
        .cursors
        .iter()
        .map(|c| (c.source_id.clone(), c.cursor_json.clone()))
        .collect();
    match st
        .db
        .upsert_source_cursors(&id.collector_id, &items, Utc::now())
    {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "cursor_sync_failed",
            &e.to_string(),
        ),
    }
}
