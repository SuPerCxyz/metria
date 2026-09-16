# Adapter 开发指南

## 1. 统一接口（metria-adapter-api）

每个 Adapter 是一个独立 crate，实现 `SourceAdapter` trait：

```rust
pub trait SourceAdapter {
    fn id(&self) -> &'static str;          // "claude-code" / "codex" / "opencode"
    fn display_name(&self) -> &'static str;
    fn version(&self) -> &'static str;
    fn capabilities(&self) -> AdapterCapabilities;
    fn discover(&self, ctx: &DiscoveryContext) -> Result<Vec<DiscoveredSource>, AdapterError>;
    fn health(&self, source: &DiscoveredSource) -> Result<SourceHealth, AdapterError>;
    fn scan(
        &self, source: &DiscoveredSource, cursor: Option<&SourceCursor>, identity: &ScanIdentity,
    ) -> Result<ScanBatch, AdapterError>;
    fn traffic_capabilities(&self) -> TrafficCapabilities;
}
```

`ScanBatch` 输出：`sessions / turns / messages / model_calls / usage_events / tool_events /
subagent_relations / traffic_estimates / traffic_profile_samples / next_cursor / warnings / source_errors`。

## 2. 已实现 Adapter

| Adapter | 数据源 | 解析要点 | 游标 |
|---|---|---|---|
| claude-code | `projects/*/*.jsonl` + 扁平布局 | modern entry（type/user/assistant/message.usage/cache_*/tool_use/tool_result/summary/ai-title）；turn 分组；Task tool_use 的 `leafUuid` 推导子代理关系 | JSONL offset+inode |
| codex | `sessions/<id>/*.jsonl` | session_meta / user_message / token_count(last_token_usage) / response_item(message/reasoning/custom_tool_call/output)；`previous_response_id` → stateful_reference；重复 token_count 去重；全零 usage 不产假调用；`input` 扣除 cached/cache_write、`output` 扣除 reasoning 归一到 Metria 口径 | JSONL offset |
| opencode | 全局 `opencode.db` + `project/*/storage/**/*.db` | 只读打开（READ_ONLY+busy_timeout+query_only，不改 PRAGMA/不 migration）；message/part(text/reasoning/tool/step)；session.cost→reported；parent_id→subagent | SQLite rowid 增量 |

### 2.1 Token 口径归一化

Adapter 必须把客户端上报的 Token 归一到 Metria 的统一口径后再落库：

- `input_tokens`：非缓存输入。客户端把缓存计入输入的（Codex），采集层减 `cached_input_tokens` 与 `cache_write_input_tokens`（不小于 0）。
- `output_tokens`：不含推理的生成 Token。客户端把推理计入输出的（Codex），采集层减 `reasoning_output_tokens`（不小于 0）。
- `reasoning_tokens`：单独计列、单独按推理单价计费，不得换算为响应字节。

总 Token 口径为 `input + output + reasoning`，缓存读写单独展示。

## 3. 解析健壮性要求（硬性）

- 容忍未知字段（`deny_unknown_fields=false`）。
- 坏记录 → warning + continue，不中断整次扫描。
- 单行长度上限；非 UTF-8 降级 `read_raw`。
- 未写完的末尾行不消费（游标停在最后完整行）。
- 时间倒序纠正标记；SQLite 支持 schema drift 检测、WAL 旧快照容忍、锁超时。

## 4. Fixture 与测试

每个 Adapter 必须有：

- **Golden Fixture**：完整事件 → 断言关键字段（session/model_call/usage/traffic 数量与取值）。
- **Malformed Fixture**：截断 JSON / 未知字段 / 非 UTF-8 / 超大行 / 重复事件 / 轮转 / 游标失效 / 锁 / Schema Drift / 时间倒序 / 负数溢出。
- 增量扫描测试：游标续扫不重复解析。

测试工具：`metria_adapter_api::testutil::{scan_fixture, scan_source, assert_golden_basics}`。

新增 Adapter 步骤：

1. `crates/metria-adapter-<name>`（独立 crate，依赖 metria-adapter-api/core/traffic）。
2. 实现 `SourceAdapter` + 容错解析。
3. 注册到 `metria-cli/src/registry.rs`。
4. 编写 fixtures + golden/malformed 测试。
5. `metria doctor --adapter <name>` 验证真实目录。

## 5. 零侵入约束（最高优先级）

- 只读挂载客户端目录，只读打开 SQLite（`query_only`），不执行 Migration。
- 禁止修改客户端配置 / API Base URL / API Key / 添加 Header。
- 禁止代理、中间人、自签名 CA、改路由/DNS/nftables、加载 eBPF、挂 Docker Socket。
- 禁止修改客户端日志或第三方数据库。
