-- 归一化 Codex 的 output_tokens：从「含推理的生成 Token」改为「不含推理的生成 Token」。
--
-- 背景：
--   Metria 约定 output 为不含推理的生成 Token，总 Token = input + output + reasoning，
--   推理单独计列并按推理单价计费。Codex 上报的 output_tokens 已包含
--   reasoning_output_tokens，导致推理被重复计入总 Token，并按输出单价重复计费。
--   此处把历史 output_tokens 扣减为生成 Token（不小于 0）；reasoning_tokens 不变。
--
-- 说明：
--   本迁移只改原始字段，费用、流量与 rollup 由启动修复（修复版本号提升）重新计算。
--   归一化后 output + reasoning 等于归一化前的 output，即推理只计一次；归一化前
--   reasoning 同时被计入 output，总 Token = input + output + reasoning 把推理算了两遍，
--   故修复后 Codex 的总 Token 会按 reasoning 的量下降（这是去重，不是数据丢失）。

UPDATE usage_events
SET output_tokens = MAX(0, COALESCE(output_tokens, 0) - COALESCE(reasoning_tokens, 0))
WHERE client_id = 'codex'
  AND output_tokens IS NOT NULL
  AND COALESCE(reasoning_tokens, 0) > 0;

UPDATE model_calls
SET output_tokens = MAX(0, COALESCE(output_tokens, 0) - COALESCE(reasoning_tokens, 0))
WHERE client_id = 'codex'
  AND output_tokens IS NOT NULL
  AND COALESCE(reasoning_tokens, 0) > 0;
