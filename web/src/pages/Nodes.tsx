import { api, q } from '../api/client'
import { Card, ErrorBox, Empty, Badge, statusTone } from '../components/ui'
import { getRange, useQuery } from '../hooks/useQuery'
import { fmtDateTime } from '../lib/format'
import { nav } from '../lib/router'

export function Nodes() {
  const nodes = useQuery<any>('/nodes', () => api('/nodes'))
  if (nodes.error) return <ErrorBox error={nodes.error} onRetry={nodes.refresh} />
  if (nodes.loading) return <Empty text="加载中…" />

  return (
    <div class="page">
      <h2>Nodes</h2>
      <Card>
        <table class="table">
          <thead>
            <tr>
              <th>Node</th>
              <th>平台</th>
              <th>架构</th>
              <th>状态</th>
              <th>最后心跳</th>
              <th>首次发现</th>
            </tr>
          </thead>
          <tbody>
            {(nodes.data?.nodes || []).map((n: any) => (
              <tr key={n.id} class="clickable" onClick={() => nav(`nodes/${n.id}`)}>
                <td>
                  {n.name}
                  <Badge text={n.id} tone="muted" />
                </td>
                <td>{n.platform || '—'}</td>
                <td>{n.architecture || '—'}</td>
                <td>
                  <Badge text={n.status} tone={statusTone(n.status)} />
                </td>
                <td>{fmtDateTime(n.last_seen_at)}</td>
                <td>{fmtDateTime(n.first_seen_at)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  )
}

export function NodeDetail({ id }: { id: string }) {
  const range = getRange()
  const params = { from: range.from, to: range.to, timezone: range.timezone }
  const node = useQuery<any>(`node${id}`, () => api(`/nodes/${encodeURIComponent(id)}`))
  const sources = useQuery<any>(`node-src${id}`, () => api(`/nodes/${encodeURIComponent(id)}/clients`))
  const sessions = useQuery<any>(`node-sess${id}${q(params)}`, () => api(`/nodes/${encodeURIComponent(id)}/sessions${q(params)}`))
  const calls = useQuery<any>(`node-calls${id}${q(params)}`, () => api(`/nodes/${encodeURIComponent(id)}/calls${q(params)}`))

  if (node.error) return <ErrorBox error={node.error} onRetry={node.refresh} />
  if (node.loading) return <Empty text="加载中…" />
  const n = node.data?.node || {}

  return (
    <div class="page">
      <button type="button" class="btn" onClick={() => nav('nodes')}>
        ← Nodes
      </button>
      <h2>Node：{n.name}</h2>
      <div class="stat-grid">
        <Card title="平台">
          <div>{n.platform || '—'} / {n.architecture || '—'}</div>
        </Card>
        <Card title="状态">
          <Badge text={n.status} tone={statusTone(n.status)} />
        </Card>
        <Card title="首次发现">{fmtDateTime(n.first_seen_at)}</Card>
        <Card title="最后心跳">{fmtDateTime(n.last_seen_at)}</Card>
      </div>

      <Card title="检测到的 Agent 工具 / Source">
        {(sources.data?.sources || []).length === 0 && <Empty text="暂无来源（Agent 尚未上报）" />}
        <table class="table">
          <thead>
            <tr>
              <th>Agent 工具</th>
              <th>Adapter</th>
              <th>版本</th>
              <th>Source Hash</th>
              <th>状态</th>
              <th>最后扫描</th>
              <th>错误</th>
            </tr>
          </thead>
          <tbody>
            {(sources.data?.sources || []).map((s: any) => (
              <tr key={s.source_path_hash}>
                <td>{s.client_id}</td>
                <td>{s.adapter_id}</td>
                <td>{s.adapter_version}</td>
                <td class="mono">{s.source_path_hash?.slice(0, 16)}…</td>
                <td>
                  <Badge text={s.status} tone={statusTone(s.status)} />
                </td>
                <td>{s.last_scan_at ? fmtDateTime(s.last_scan_at) : '—'}</td>
                <td>{s.last_error || '—'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>

      <div class="grid-2">
        <Card title="最近 Sessions">
          {(sessions.data?.sessions || []).map((s: any) => (
            <div key={s.id} class="list-row clickable" onClick={() => nav(`sessions/${s.id}`)}>
              <span>{s.title || s.client_id}</span>
              <span>{fmtDateTime(s.started_at)} · {s.model_call_count} calls</span>
            </div>
          ))}
        </Card>
        <Card title="最近 Model Calls">
          {(calls.data?.calls || []).map((c: any) => (
            <div key={c.id} class="list-row clickable" onClick={() => nav(`calls/${c.id}`)}>
              <span>{c.model || '—'}</span>
              <span>{fmtDateTime(c.started_at)}</span>
            </div>
          ))}
        </Card>
      </div>
    </div>
  )
}
