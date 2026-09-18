//! 运行时观测的有界派生指标。

use std::time::Instant;

use chrono::{DateTime, Utc};
use serde_json::Value;

pub(crate) const MAX_LINE_BYTES: usize = 16 * 1024;
const MAX_NON_STREAM_BODY: usize = 2 * 1024 * 1024;
const STALL_THRESHOLD_MS: i64 = 1_000;

/// 一次请求的派生观测指标；不保存请求/响应正文。
#[derive(Debug, Clone, Default)]
pub(crate) struct ObservedCall {
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) first_byte_at: Option<DateTime<Utc>>,
    pub(crate) first_token_at: Option<DateTime<Utc>>,
    pub(crate) last_output_at: Option<DateTime<Utc>>,
    pub(crate) completed_at: Option<DateTime<Utc>>,
    pub(crate) status_code: Option<i64>,
    pub(crate) streaming: bool,
    pub(crate) stream_completed: bool,
    pub(crate) input_tokens: Option<i64>,
    pub(crate) output_tokens: Option<i64>,
    pub(crate) cache_read_tokens: Option<i64>,
    pub(crate) cache_write_tokens: Option<i64>,
    pub(crate) reasoning_tokens: Option<i64>,
    pub(crate) model: Option<String>,
    pub(crate) finish_reason: Option<String>,
    pub(crate) error_kind: Option<String>,
    pub(crate) rate_limited: Option<bool>,
    pub(crate) endpoint: Option<String>,
    pub(crate) observed_request_payload_bytes: Option<i64>,
    pub(crate) observed_response_payload_bytes: Option<i64>,
    pub(crate) observed_request_wire_bytes: Option<i64>,
    pub(crate) observed_response_wire_bytes: Option<i64>,
    pub(crate) inter_token_latency_avg_ms: Option<i64>,
    pub(crate) inter_token_latency_p95_ms: Option<i64>,
    pub(crate) stall_count: Option<i64>,
    pub(crate) stall_duration_ms: Option<i64>,
}

pub(crate) struct CallIdentity<'a> {
    pub(crate) id: &'a str,
    pub(crate) node_id: &'a str,
    pub(crate) collector_id: &'a str,
    pub(crate) source_id: &'a str,
    pub(crate) source_session_id: &'a str,
    pub(crate) session_id: &'a str,
    pub(crate) client_id: &'a str,
}

impl ObservedCall {
    pub(crate) fn new(started_at: DateTime<Utc>, endpoint: String, model: Option<String>) -> Self {
        Self {
            started_at,
            endpoint: Some(endpoint),
            model,
            ..Self::default()
        }
    }

    pub(crate) fn to_payload(&self, identity: &CallIdentity<'_>) -> Value {
        let completed = self.completed_at;
        let duration_ms = completed.map(|at| (at - self.started_at).num_milliseconds());
        let first_byte_latency_ms = self
            .first_byte_at
            .map(|at| (at - self.started_at).num_milliseconds());
        let ttft_ms = self
            .first_token_at
            .map(|at| (at - self.started_at).num_milliseconds());
        let generation_duration_ms = self
            .first_token_at
            .zip(self.last_output_at)
            .map(|(first, last)| (last - first).num_milliseconds())
            .filter(|v| *v > 0);
        let output_tokens_per_second_milli = self
            .output_tokens
            .zip(generation_duration_ms)
            .and_then(|(tokens, duration)| {
                if tokens > 0 && duration > 0 {
                    Some(((tokens as i128) * 1_000_000 / duration as i128) as i64)
                } else {
                    None
                }
            });
        let status = if self.status_code.unwrap_or(500) < 400 {
            "success"
        } else {
            "error"
        };
        serde_json::json!({
            "id": identity.id,
            "source_call_id": identity.id,
            "node_id": identity.node_id,
            "collector_id": identity.collector_id,
            "client_id": identity.client_id,
            "source_id": identity.source_id,
            "source_session_id": identity.source_session_id,
            "session_id": identity.session_id,
            "provider_raw": identity.client_id,
            "provider_normalized": identity.client_id,
            "model_raw": self.model,
            "model_normalized": self.model,
            "started_at": self.started_at,
            "first_byte_at": self.first_byte_at,
            "first_token_at": self.first_token_at,
            "first_response_at": self.first_token_at.or(self.first_byte_at),
            "last_output_at": self.last_output_at,
            "completed_at": completed,
            "duration_ms": duration_ms,
            "first_byte_latency_ms": first_byte_latency_ms,
            "ttft_ms": ttft_ms,
            "generation_duration_ms": generation_duration_ms,
            "output_tokens_per_second_milli": output_tokens_per_second_milli,
            "inter_token_latency_avg_ms": self.inter_token_latency_avg_ms,
            "inter_token_latency_p95_ms": self.inter_token_latency_p95_ms,
            "stall_count": self.stall_count,
            "stall_duration_ms": self.stall_duration_ms,
            "timing_source": "runtime_http",
            "timing_quality": if self.first_token_at.is_some() { "observed" } else { "partial" },
            "observability_source": "runtime_http",
            "observability_quality": if self.first_token_at.is_some() { "observed" } else { "partial" },
            "status": status,
            "status_code": self.status_code,
            "streaming": self.streaming,
            "stream_completed": self.stream_completed,
            "client_aborted": false,
            "retry_count": 0,
            "call_granularity": "call",
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
            "cache_read_tokens": self.cache_read_tokens,
            "cache_write_tokens": self.cache_write_tokens,
            "reasoning_tokens": self.reasoning_tokens,
            "endpoint": self.endpoint,
            "finish_reason": self.finish_reason,
            "error_kind": self.error_kind,
            "rate_limited": self.rate_limited,
            "observed_request_payload_bytes": self.observed_request_payload_bytes,
            "observed_response_payload_bytes": self.observed_response_payload_bytes,
            "observed_request_wire_bytes": self.observed_request_wire_bytes,
            "observed_response_wire_bytes": self.observed_response_wire_bytes,
            "usage_event_id": Value::Null,
            "traffic_estimate_id": Value::Null,
            "created_at": self.started_at,
            "updated_at": completed.unwrap_or(self.started_at),
        })
    }
}

#[derive(Debug)]
pub(crate) struct Tracker {
    pub(crate) call: ObservedCall,
    line: Vec<u8>,
    non_stream_body: Vec<u8>,
    output_events: Vec<i64>,
    last_output_instant: Option<Instant>,
    inter_token_sum: i64,
    stall_count: i64,
    stall_duration_ms: i64,
    pub(crate) request_payload_bytes: Option<i64>,
    pub(crate) request_wire_bytes: Option<i64>,
    pub(crate) response_payload_bytes: Option<i64>,
    pub(crate) response_wire_bytes: Option<i64>,
}

impl Tracker {
    pub(crate) fn new(started_at: DateTime<Utc>, endpoint: String, model: Option<String>) -> Self {
        Self {
            call: ObservedCall::new(started_at, endpoint, model),
            line: Vec::new(),
            non_stream_body: Vec::new(),
            output_events: Vec::new(),
            last_output_instant: None,
            inter_token_sum: 0,
            stall_count: 0,
            stall_duration_ms: 0,
            request_payload_bytes: None,
            request_wire_bytes: None,
            response_payload_bytes: None,
            response_wire_bytes: None,
        }
    }

    pub(crate) fn feed(&mut self, bytes: &[u8]) {
        if !self.call.streaming && self.non_stream_body.len() < MAX_NON_STREAM_BODY {
            let remaining = MAX_NON_STREAM_BODY - self.non_stream_body.len();
            self.non_stream_body
                .extend_from_slice(&bytes[..bytes.len().min(remaining)]);
        }
        for byte in bytes {
            if *byte == b'\n' {
                let line = std::mem::take(&mut self.line);
                self.feed_line(&line);
            } else if self.line.len() < MAX_LINE_BYTES {
                self.line.push(*byte);
            }
        }
    }

    fn feed_line(&mut self, line: &[u8]) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(data) = line.strip_prefix(b"data:") else {
            return;
        };
        let data = data
            .iter()
            .copied()
            .skip_while(|byte| byte.is_ascii_whitespace())
            .collect::<Vec<_>>();
        if data == b"[DONE]" {
            return;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&data) else {
            return;
        };
        self.call.streaming = true;
        observe_json(&value, &mut self.call);
        if has_output_delta(&value) {
            self.mark_output();
        }
    }

    fn mark_output(&mut self) {
        let now = Instant::now();
        let at = Utc::now();
        if self.call.first_token_at.is_none() {
            self.call.first_token_at = Some(at);
        }
        self.call.last_output_at = Some(at);
        if let Some(previous) = self.last_output_instant {
            let interval = previous.elapsed().as_millis().min(i64::MAX as u128) as i64;
            self.output_events.push(interval);
            self.inter_token_sum = self.inter_token_sum.saturating_add(interval);
            if interval > STALL_THRESHOLD_MS {
                self.stall_count += 1;
                self.stall_duration_ms = self
                    .stall_duration_ms
                    .saturating_add(interval - STALL_THRESHOLD_MS);
            }
        }
        self.last_output_instant = Some(now);
    }

    pub(crate) fn finish(mut self) -> ObservedCall {
        if !self.call.streaming && !self.non_stream_body.is_empty() {
            if let Ok(value) = serde_json::from_slice::<Value>(&self.non_stream_body) {
                observe_json(&value, &mut self.call);
            }
        }
        if !self.output_events.is_empty() {
            self.output_events.sort_unstable();
            let count = self.output_events.len() as i64;
            self.call.inter_token_latency_avg_ms = Some(self.inter_token_sum / count);
            let index = ((self.output_events.len() as f64) * 0.95).ceil() as usize;
            self.call.inter_token_latency_p95_ms = self
                .output_events
                .get(index.saturating_sub(1).min(self.output_events.len() - 1))
                .copied();
            self.call.stall_count = Some(self.stall_count);
            self.call.stall_duration_ms = Some(self.stall_duration_ms);
        }
        self.call.observed_request_payload_bytes = self.request_payload_bytes;
        self.call.observed_request_wire_bytes = self.request_wire_bytes;
        self.call.observed_response_payload_bytes = self.response_payload_bytes;
        self.call.observed_response_wire_bytes = self.response_wire_bytes;
        self.call
    }
}

pub(crate) fn observe_json(value: &Value, call: &mut ObservedCall) {
    match value {
        Value::Object(map) => {
            if let Some(model) = map.get("model").and_then(Value::as_str) {
                call.model = Some(model.to_string());
            }
            if let Some(reason) = map
                .get("finish_reason")
                .or_else(|| map.get("stop_reason"))
                .and_then(Value::as_str)
            {
                call.finish_reason = Some(reason.to_string());
            }
            for (key, value) in map {
                match key.as_str() {
                    "input_tokens" | "prompt_tokens" => set_if_some(&mut call.input_tokens, value),
                    "output_tokens" | "completion_tokens" => {
                        set_if_some(&mut call.output_tokens, value)
                    }
                    "cache_read_input_tokens" | "cached_tokens" | "cache_read" => {
                        set_if_some(&mut call.cache_read_tokens, value)
                    }
                    "cache_creation_input_tokens" | "cache_write" => {
                        set_if_some(&mut call.cache_write_tokens, value)
                    }
                    "reasoning_tokens" | "reasoning_output_tokens" => {
                        set_if_some(&mut call.reasoning_tokens, value)
                    }
                    _ => {}
                }
                observe_json(value, call);
            }
        }
        Value::Array(items) => {
            for item in items {
                observe_json(item, call);
            }
        }
        _ => {}
    }
}

fn set_if_some(target: &mut Option<i64>, value: &Value) {
    if let Some(number) = value.as_i64().filter(|n| *n >= 0) {
        *target = Some(number);
    }
}

pub(crate) fn has_output_delta(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            matches!(
                key.as_str(),
                "content" | "text" | "thinking" | "reasoning_content"
            ) && value.as_str().is_some_and(|text| !text.is_empty())
                || has_output_delta(value)
        }),
        Value::Array(items) => items.iter().any(has_output_delta),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_sse_tracker_derives_timing_without_content_storage() {
        let mut tracker = Tracker::new(
            Utc::now(),
            "https://example.test/v1/chat".into(),
            Some("model".into()),
        );
        tracker.feed(
            br#"data: {"choices":[{"delta":{"content":"secret"}}]}
"#,
        );
        tracker.feed(
            br#"data: {"choices":[{"delta":{"content":"more"}}]}
data: [DONE]
"#,
        );
        let call = tracker.finish();
        assert!(call.first_token_at.is_some());
        assert!(call.last_output_at.is_some());
        assert!(call.inter_token_latency_avg_ms.is_some());
        let debug = format!("{call:?}");
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("more"));
    }

    #[test]
    fn non_stream_body_keeps_usage_but_not_stream_timing() {
        let mut tracker = Tracker::new(Utc::now(), "https://example.test/v1".into(), None);
        tracker.feed(br#"{"usage":{"prompt_tokens":4,"completion_tokens":2}}"#);
        let call = tracker.finish();
        assert_eq!(call.input_tokens, Some(4));
        assert_eq!(call.output_tokens, Some(2));
        assert!(call.first_token_at.is_none());
        assert!(call.inter_token_latency_avg_ms.is_none());
    }
}
