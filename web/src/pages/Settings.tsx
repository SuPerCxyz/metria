import { api, q } from '../api/client'
import { Card, Empty } from '../components/ui'
import { getRange, useQuery } from '../hooks/useQuery'
import { fmtBytes } from '../lib/format'

/** 设置：导出、数据概览与运维信息。 */
export function Settings() {
  const range = getRange()
  const overview = useQuery<any>(
    `overview${q({ from: range.from, to: range.to })}`,
    () => api(`/overview${q({ from: range.from, to: range.to })}`),
  )
  const exportBase = `/api/v1/export`

  const dl = (kind: string, fmt: string) =>
    `${exportBase}?kind=${kind}&format=${fmt}&from=${encodeURIComponent(range.from)}&to=${encodeURIComponent(range.to)}`

  return (
    <div class="page">
      <h2>设置</h2>
      <div class="grid-2">
        <Card title="数据导出">
          <p class="page-note">导出当前时间范围内的数据（JSON / NDJSON / CSV）。</p>
          <div class="dim-switch">
            <a class="btn small" href={dl('sessions', 'json')}>
              会话 JSON
            </a>
            <a class="btn small" href={dl('sessions', 'csv')}>
              会话 CSV
            </a>
            <a class="btn small" href={dl('calls', 'ndjson')}>
              调用 NDJSON
            </a>
            <a class="btn small" href={dl('calls', 'csv')}>
              调用 CSV
            </a>
          </div>
        </Card>

        <Card title="数据概览（当前时间范围）">
          {overview.data ? (
            <div class="kv-grid">
              {[
                ['Model Calls', String(overview.data.model_calls ?? 0)],
                ['Sessions', String(overview.data.sessions ?? 0)],
                ['Input Tokens', String(overview.data.input_tokens ?? 0)],
                ['Output Tokens', String(overview.data.output_tokens ?? 0)],
                ['估算流量', fmtBytes(overview.data.estimated_total_bytes)],
                ['Nodes', String(overview.data.nodes ?? 0)],
              ].map(([k, v]) => (
                <div class="kv" key={k}>
                  <span class="kv-label">{k}</span>
                  <span class="kv-value">{v}</span>
                </div>
              ))}
            </div>
          ) : (
            <Empty text="加载中…" />
          )}
        </Card>

        <Card title="数据保留策略">
          <p class="page-note">
            默认全量保留原始事件与 rollup。备份 / 恢复 / 升级 / 回滚见
            <code> docs/operations.md</code>。
          </p>
          <ul style="padding-left:18px;color:var(--fg-muted);font-size:13px">
            <li>usage_events / model_calls / sessions 永久保留</li>
            <li>hourly_rollups / daily_rollups 永久保留（Dashboard 读取）</li>
            <li>traffic_estimates 永久保留（重新估算保留新旧版本）</li>
            <li>内容模式 content_mode：none / metadata / full</li>
          </ul>
        </Card>

        <Card title="MCP 只读查询">
          <p class="page-note">
            通过 <code>metria mcp</code> 提供 stdio 只读查询工具：
            overview / list_nodes / list_models / list_sessions / get_session / list_calls / traffic_summary。
          </p>
        </Card>
      </div>
    </div>
  )
}
