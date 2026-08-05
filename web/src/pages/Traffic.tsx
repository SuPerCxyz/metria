import { api, q } from '../api/client'
import { Card, ErrorBox, Empty, StatCard } from '../components/ui'
import { TimeSeries } from '../components/TimeSeries'
import { getRange, useQuery } from '../hooks/useQuery'
import { fmtBytes } from '../lib/format'

const DIM_LABELS: Record<string, string> = {
  node_id: 'Node',
  client_id: 'Agent 工具',
  model: '模型',
  provider: 'Provider',
}

export function Traffic() {
  const range = getRange()
  const params = { from: range.from, to: range.to, timezone: range.timezone }
  const summary = useQuery<any>(`traffic-sum${q(params)}`, () => api(`/traffic/summary${q(params)}`))
  const series = useQuery<any>(`traffic-ts${q(params)}`, () => api(`/usage/timeseries${q(params)}`))
  const [dim, setDim] = useState_local('node_id')
  const byDim = useQuery<any>(`traffic-by${dim}${q(params)}`, () => api(`/traffic/by-${dim}${q(params)}`))

  if (summary.error) return <ErrorBox error={summary.error} onRetry={summary.refresh} />
  if (summary.loading) return <Empty text="加载中…" />
  const s = summary.data || {}

  const trafficSeries = {
    ...series,
    data: (series.data?.series || []).map((p: any) => ({ bucket: p.bucket, value: p.estimated_traffic_bytes })),
  }

  return (
    <div class="page">
      <h2>估算流量</h2>
      <p class="page-note">
        这些数据是根据客户端日志和 Token 估算，不代表网卡真实流量或云厂商计费流量。
      </p>
      <div class="stat-grid">
        <StatCard label="估算请求流量" value={fmtBytes(s.estimated_request_bytes)} />
        <StatCard label="估算响应流量" value={fmtBytes(s.estimated_response_bytes)} />
        <StatCard label="估算总流量" value={fmtBytes(s.estimated_total_bytes)} accent="#2563eb" />
        <StatCard label="下界" value={fmtBytes(s.lower_bound_bytes)} />
        <StatCard label="上界" value={fmtBytes(s.upper_bound_bytes)} />
        <StatCard label="Model Calls" value={String(s.model_calls ?? 0)} />
      </div>

      <Card title="估算流量时间序列">
        {trafficSeries.data?.length ? <TimeSeries data={trafficSeries.data} height={200} /> : <Empty />}
      </Card>

      <Card
        title={`按维度汇总：${DIM_LABELS[dim] || dim}`}
      >
        <div class="dim-switch">
          {Object.entries(DIM_LABELS).map(([k, label]) => (
            <button type="button" key={k} class={`btn small ${dim === k ? 'primary' : ''}`} onClick={() => setDim(k)}>
              {label}
            </button>
          ))}
        </div>
        <table class="table">
          <thead>
            <tr>
              <th>{DIM_LABELS[dim] || dim}</th>
              <th>Calls</th>
              <th>请求流量</th>
              <th>响应流量</th>
              <th>总流量</th>
              <th>下界</th>
              <th>上界</th>
            </tr>
          </thead>
          <tbody>
            {(byDim.data?.items || []).map((it: any) => (
              <tr key={it.dimension}>
                <td>{it.dimension}</td>
                <td>{it.model_calls}</td>
                <td>{fmtBytes(it.estimated_request_bytes)}</td>
                <td>{fmtBytes(it.estimated_response_bytes)}</td>
                <td>{fmtBytes(it.estimated_total_bytes)}</td>
                <td>{fmtBytes(it.lower_bound_bytes)}</td>
                <td>{fmtBytes(it.upper_bound_bytes)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  )
}

import { useState } from 'preact/hooks'
function useState_local<T>(init: T): [T, (v: T) => void] {
  const [v, setV] = useState<T>(init)
  return [v, setV]
}
