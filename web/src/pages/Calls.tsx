import { api, q } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { getRange, useQuery } from '../hooks/useQuery'
import { fmtBytes, fmtDateTime, fmtTokens, fmtUsd } from '../lib/format'
import { nav } from '../lib/router'

export function Calls() {
  const range = getRange()
  const params = { from: range.from, to: range.to, timezone: range.timezone }
  const calls = useQuery<any>(`calls${q(params)}`, () => api(`/calls${q(params)}`))
  if (calls.error) return <ErrorBox error={calls.error} onRetry={calls.refresh} />
  if (calls.loading) return <Empty text="加载中…" />

  return (
    <div class="page">
      <h2>Model Calls</h2>
      <Card>
        <table class="table">
          <thead>
            <tr>
              <th>时间</th>
              <th>Client</th>
              <th>模型</th>
              <th>Provider</th>
              <th>状态</th>
              <th>Input</th>
              <th>Output</th>
              <th>Cache</th>
              <th>费用</th>
            </tr>
          </thead>
          <tbody>
            {(calls.data?.calls || []).map((c: any) => (
              <tr key={c.id} class="clickable" onClick={() => nav(`calls/${c.id}`)}>
                <td>{fmtDateTime(c.started_at)}</td>
                <td>{c.client_id}</td>
                <td>{c.model || '—'}</td>
                <td>{c.provider || '—'}</td>
                <td>{c.status}</td>
                <td>{fmtTokens(c.input_tokens)}</td>
                <td>{fmtTokens(c.output_tokens)}</td>
                <td>{fmtTokens((c.cache_read_tokens ?? 0) + (c.cache_write_tokens ?? 0))}</td>
                <td>{fmtUsd(c.calculated_cost_micro_usd ?? c.reported_cost_micro_usd)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  )
}

export function CallDetail({ id }: { id: string }) {
  const call = useQuery<any>(`call${id}`, () => api(`/calls/${encodeURIComponent(id)}`))
  if (call.error) return <ErrorBox error={call.error} onRetry={call.refresh} />
  if (call.loading) return <Empty text="加载中…" />
  const c = call.data?.call || {}
  const t = call.data?.traffic || {}

  return (
    <div class="page">
      <button type="button" class="btn" onClick={() => nav('calls')}>
        ← Calls
      </button>
      <h2>Call：{c.id}</h2>
      <div class="stat-grid">
        {[
          ['Client', c.client_id],
          ['Session', c.session_id || '—'],
          ['模型', c.model_raw || c.model || '—'],
          ['Provider', c.provider_raw || c.provider || '—'],
          ['开始', fmtDateTime(c.started_at)],
          ['完成', c.completed_at ? fmtDateTime(c.completed_at) : '—'],
          ['状态', c.status],
          ['粒度', c.call_granularity],
          ['Input', fmtTokens(c.input_tokens)],
          ['Output', fmtTokens(c.output_tokens)],
          ['Cache Read', fmtTokens(c.cache_read_tokens)],
          ['Reasoning', fmtTokens(c.reasoning_tokens)],
          ['Reported Cost', fmtUsd(c.reported_cost_micro_usd)],
          ['Calculated Cost', fmtUsd(c.calculated_cost_micro_usd)],
        ].map(([label, value]) => (
          <div class="kv" key={label}>
            <span class="kv-label">{label}</span>
            <span class="kv-value">{value}</span>
          </div>
        ))}
      </div>

      <Card title="估算流量">
        <div class="traffic-display">
          <div class="traffic-main">
            估算流量：{fmtBytes(t.estimated_total_wire_bytes)}
            <span class="traffic-range">
              估算范围：{fmtBytes(t.lower_bound_bytes)} ~ {fmtBytes(t.upper_bound_bytes)}
            </span>
          </div>
          <div class="kv-grid">
            {[
              ['请求流量', fmtBytes(t.estimated_request_wire_bytes)],
              ['响应流量', fmtBytes(t.estimated_response_wire_bytes)],
              ['可信度', t.confidence != null ? `${Math.round(t.confidence * 100)}%` : '—'],
              ['估算来源', t.estimation_source],
              ['上下文传输', t.context_transport_mode],
              ['Cache 行为', t.cache_transport_behavior],
            ].map(([label, value]) => (
              <div class="kv" key={label}>
                <span class="kv-label">{label}</span>
                <span class="kv-value">{value}</span>
              </div>
            ))}
          </div>
          <p class="traffic-note">
            提示：以上为根据客户端日志与 Token 估算的「估算流量」，不代表网卡真实流量或云厂商计费流量。
          </p>
        </div>
      </Card>
    </div>
  )
}
