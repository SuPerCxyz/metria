import { api, q } from '../api/client'
import { Card, StatCard, ErrorBox, Empty } from '../components/ui'
import { TimeSeries } from '../components/TimeSeries'
import { getRange } from '../hooks/useQuery'
import { useQuery } from '../hooks/useQuery'
import { fmtBytes, fmtTokens, fmtUsd, fmtDateTime } from '../lib/format'
import { nav } from '../lib/router'

export function Overview() {
  const range = getRange()
  const params = {
    from: range.from,
    to: range.to,
    timezone: range.timezone,
  }

  const overview = useQuery<any>(`overview${q(params)}`, () => api(`/overview${q(params)}`))
  const series = useQuery<any>(`ts${q(params)}`, () => api(`/usage/timeseries${q(params)}`))
  const byNode = useQuery<any>(`breakdown${q(params)}`, () => api(`/usage/breakdown${q(params)}`))
  const recentSessions = useQuery<any>(
    `sessions${q({ ...params, limit: 8 })}`,
    () => api(`/sessions${q({ ...params, limit: 8 })}`),
  )

  if (overview.error) return <ErrorBox error={overview.error} onRetry={overview.refresh} />
  if (overview.loading) return <Empty text="加载中…" />
  const o = overview.data || {}

  const tokSeries = {
    ...series,
    data: (series.data?.series || []).map((p: any) => ({ bucket: p.bucket, value: p.input_tokens + p.output_tokens })),
  }
  const costSeries = {
    ...series,
    data: (series.data?.series || []).map((p: any) => ({ bucket: p.bucket, value: p.cost_micro_usd })),
  }
  const trafficSeries = {
    ...series,
    data: (series.data?.series || []).map((p: any) => ({ bucket: p.bucket, value: p.estimated_traffic_bytes })),
  }

  return (
    <div class="page">
      <h2>总览</h2>
      <div class="stat-grid">
        <StatCard label="Input Tokens" value={fmtTokens(o.input_tokens)} />
        <StatCard label="Output Tokens" value={fmtTokens(o.output_tokens)} />
        <StatCard label="Cache Read" value={fmtTokens(o.cache_read_tokens)} />
        <StatCard label="Cache Write" value={fmtTokens(o.cache_write_tokens)} />
        <StatCard label="Reasoning" value={fmtTokens(o.reasoning_tokens)} />
        <StatCard label="Reported Cost" value={fmtUsd(o.reported_cost_micro_usd)} />
        <StatCard label="Calculated Cost" value={fmtUsd(o.calculated_cost_micro_usd)} />
        <StatCard label="Estimated Cost" value={fmtUsd(o.estimated_cost_micro_usd)} />
        <StatCard label="估算流量" value={fmtBytes(o.estimated_total_bytes)} sub={o.traffic_lower_bound_bytes != null ? `范围 ${fmtBytes(o.traffic_lower_bound_bytes)} ~ ${fmtBytes(o.traffic_upper_bound_bytes)}` : undefined} accent="#2563eb" />
        <StatCard label="Model Calls" value={fmtTokens(o.model_calls)} />
        <StatCard label="Sessions" value={fmtTokens(o.sessions)} />
        <StatCard label="活跃 Nodes" value={String(o.nodes)} />
        <StatCard label="活跃 Collectors" value={String(o.collectors)} />
      </div>

      <div class="grid-2">
        <Card title="Token 时间序列">
          {tokSeries.data?.length ? <TimeSeries data={tokSeries.data} /> : <Empty />}
        </Card>
        <Card title="Cost 时间序列">
          {costSeries.data?.length ? <TimeSeries data={costSeries.data} /> : <Empty />}
        </Card>
        <Card title="估算流量时间序列">
          {trafficSeries.data?.length ? <TimeSeries data={trafficSeries.data} /> : <Empty />}
        </Card>
        <Card title="按 Node 汇总">
          <table class="table">
            <thead>
              <tr>
                <th>Node</th>
                <th>Input</th>
                <th>Output</th>
                <th>估算流量</th>
                <th>Calls</th>
              </tr>
            </thead>
            <tbody>
              {(byNode.data?.by_node || []).map((n: any) => (
                <tr key={n.node_id} class="clickable" onClick={() => nav(`nodes/${n.node_id}`)}>
                  <td>{n.node_id}</td>
                  <td>{fmtTokens(n.input_tokens)}</td>
                  <td>{fmtTokens(n.output_tokens)}</td>
                  <td>{fmtBytes(n.estimated_traffic_bytes)}</td>
                  <td>{n.model_calls}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      </div>

      <Card title="最近 Session">
        <table class="table">
          <thead>
            <tr>
              <th>标题</th>
              <th>Agent 工具</th>
              <th>开始</th>
              <th>Calls</th>
              <th>估算流量</th>
            </tr>
          </thead>
          <tbody>
            {(recentSessions.data?.sessions || []).map((s: any) => (
              <tr key={s.id} class="clickable" onClick={() => nav(`sessions/${s.id}`)}>
                <td>{s.title || s.source_session_id}</td>
                <td>{s.client_id}</td>
                <td>{fmtDateTime(s.started_at)}</td>
                <td>{s.model_call_count}</td>
                <td>{fmtBytes(s.estimated_total_bytes)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  )
}
