-- 原生 Agent 运行时模型调用观测：全部可空，Docker/旧 Agent 无需填充。
ALTER TABLE model_calls ADD COLUMN first_byte_at TEXT;
ALTER TABLE model_calls ADD COLUMN first_token_at TEXT;
ALTER TABLE model_calls ADD COLUMN last_output_at TEXT;
ALTER TABLE model_calls ADD COLUMN first_byte_latency_ms INTEGER;
ALTER TABLE model_calls ADD COLUMN ttft_ms INTEGER;
ALTER TABLE model_calls ADD COLUMN generation_duration_ms INTEGER;
ALTER TABLE model_calls ADD COLUMN output_tokens_per_second_milli INTEGER;
ALTER TABLE model_calls ADD COLUMN inter_token_latency_avg_ms INTEGER;
ALTER TABLE model_calls ADD COLUMN inter_token_latency_p95_ms INTEGER;
ALTER TABLE model_calls ADD COLUMN stall_count INTEGER;
ALTER TABLE model_calls ADD COLUMN stall_duration_ms INTEGER;
ALTER TABLE model_calls ADD COLUMN observability_source TEXT;
ALTER TABLE model_calls ADD COLUMN observability_quality TEXT;
ALTER TABLE model_calls ADD COLUMN endpoint TEXT;
ALTER TABLE model_calls ADD COLUMN finish_reason TEXT;
ALTER TABLE model_calls ADD COLUMN error_kind TEXT;
ALTER TABLE model_calls ADD COLUMN rate_limited INTEGER;
ALTER TABLE model_calls ADD COLUMN observed_request_payload_bytes INTEGER;
ALTER TABLE model_calls ADD COLUMN observed_response_payload_bytes INTEGER;
ALTER TABLE model_calls ADD COLUMN observed_request_wire_bytes INTEGER;
ALTER TABLE model_calls ADD COLUMN observed_response_wire_bytes INTEGER;

CREATE INDEX IF NOT EXISTS idx_calls_observability
    ON model_calls(observability_source, started_at);
