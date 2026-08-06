import { api, q } from '../api/client'
import { Card, ErrorBox, Empty, Badge, statusTone, StatCard } from '../components/ui'
import { getRange, useQuery } from '../hooks/useQuery'
import { fmtBytes, fmtDateTime, fmtTokens, fmtUsd } from '../lib/format'
import { t } from '../lib/i18n'
import { nav } from '../lib/router'

export function Nodes() {
  const nodes = useQuery<any>('/nodes', () => api('/nodes'))
  if (nodes.error) return <ErrorBox error={nodes.error} onRetry={nodes.refresh} />
  if (nodes.loading) return <Empty text={t('common.loading')} />

  return (
    <div class="page">
      <h2>Nodes</h2>
      <Card>
        <table class="table">
          <thead>
            <tr>
              <th>Node</th>
              <th>{t('nodes.platform')}</th>
              <th>架构</th>
              <th>{t('nodes.detectedClients')}</th>
              <th>{t('common.status')}</th>
              <th>{t('nodes.lastHeartbeat')}</th>
              <th>{t('nodes.firstSeen')}</th>
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
                  <Badge text={`${n.detected_clients ?? 0} ${t('nodes.detected')}`} tone="muted" />
                </td>
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
  const node = useQuery<any>(`node${id}`, () => api(`/nodes/${encodeURIComponent(id)}${q(params)}`))
  const sources = useQuery<any>(`node-src${id}`, () => api(`/nodes/${encodeURIComponent(id)}/clients`))
  const sessions = useQuery<any>(`node-sess${id}${q(params)}`, () => api(`/nodes/${encodeURIComponent(id)}/sessions${q(params)}`))
  const calls = useQuery<any>(`node-calls${id}${q(params)}`, () => api(`/nodes/${encodeURIComponent(id)}/calls${q(params)}`))

  if (node.error) return <ErrorBox error={node.error} onRetry={node.refresh} />
  if (node.loading) return <Empty text={t('common.loading')} />
  const n = node.data?.node || {}

  return (
    <div class="page">
      <button type="button" class="btn" onClick={() => nav('nodes')}>
        ← Nodes
      </button>
      <h2>Node：{n.name}</h2>
      <div class="stat-grid">
        <Card title={t('nodes.platform')}>
          <div>{n.platform || '—'} / {n.architecture || '—'}</div>
        </Card>
        <Card title={t('common.status')}>
          <Badge text={n.status} tone={statusTone(n.status)} />
        </Card>
        <Card title={t('nodes.firstSeen')}>{fmtDateTime(n.first_seen_at)}</Card>
        <Card title={t('nodes.lastHeartbeat')}>{fmtDateTime(n.last_seen_at)}</Card>
      </div>

      {(n.range_summary && Object.keys(n.range_summary).length > 0 && (
        <Card title={t('nodes.rangeStats')}>
          <div class="stat-grid">
            <StatCard label="Input" value={fmtTokens(n.range_summary.input_tokens)} />
            <StatCard label="Output" value={fmtTokens(n.range_summary.output_tokens)} />
            <StatCard label={t('common.cost')} value={fmtUsd(n.range_summary.cost_micro_usd)} />
            <StatCard label={t('common.estimatedTraffic')} value={fmtBytes(n.range_summary.estimated_total_bytes)} />
            <StatCard label={t('common.modelCalls')} value={fmtTokens(n.range_summary.model_calls)} />
            <StatCard label={t('common.sessions')} value={fmtTokens(n.range_summary.sessions)} />
          </div>
        </Card>
      ))}

      <Card title={t('nodes.collectors')}>
        <table class="table">
          <thead>
            <tr>
              <th>{t('nodes.collectorId')}</th>
              <th>Agent 版本</th>
              <th>{t('common.status')}</th>
              <th>{t('nodes.lastHeartbeat')}</th>
              <th>{t('nodes.lastUpload')}</th>
              <th>Spool</th>
              <th>{t('nodes.clockSkew')}</th>
            </tr>
          </thead>
          <tbody>
            {(n.collectors || []).map((col: any) => (
              <tr key={col.id}>
                <td class="mono">{col.id}</td>
                <td>{col.agent_version}</td>
                <td>
                  <Badge text={col.status} tone={statusTone(col.status)} />
                </td>
                <td>{fmtDateTime(col.last_heartbeat_at)}</td>
                <td>{col.last_upload_at ? fmtDateTime(col.last_upload_at) : '—'}</td>
                <td>{col.spool_pending_events} ev / {fmtBytes(col.spool_size_bytes)}</td>
                <td>{col.clock_skew_seconds}s</td>
              </tr>
            ))}
            {(n.collectors || []).length === 0 && (
              <tr>
                <td colSpan={7}>{t('common.empty')}</td>
              </tr>
            )}
          </tbody>
        </table>
      </Card>

      <Card title={t('nodes.detected')}>
        {(sources.data?.sources || []).length === 0 && <Empty text={t('client.sources')} />}
        <table class="table">
          <thead>
            <tr>
              <th>{t('sessions.client')}</th>
              <th>Adapter</th>
              <th>版本</th>
              <th>Source Hash</th>
              <th>{t('common.status')}</th>
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

      <Card title={t('nodes.byClient')}>
        <table class="table">
          <thead>
            <tr>
              <th>{t('sessions.client')}</th>
              <th>{t('nodes.sources')}</th>
              <th>{t('common.modelCalls')}</th>
            </tr>
          </thead>
          <tbody>
            {(n.clients || []).map((cl: any) => (
              <tr key={cl.client_id} class="clickable" onClick={() => nav(`clients/${cl.client_id}`)}>
                <td>{cl.client_id}</td>
                <td>{cl.source_count}</td>
                <td>{cl.model_calls ?? '—'}</td>
              </tr>
            ))}
            {(n.clients || []).length === 0 && (
              <tr>
                <td colSpan={3}>{t('common.empty')}</td>
              </tr>
            )}
          </tbody>
        </table>
      </Card>

      <div class="grid-2">
        <Card title={t('nodes.byModel')}>
          <table class="table">
            <thead>
              <tr>
                <th>{t('common.model')}</th>
                <th>Calls</th>
                <th>Input</th>
                <th>Output</th>
              </tr>
            </thead>
            <tbody>
              {(n.by_model || []).map((m: any) => (
                <tr key={m.model}>
                  <td>{m.model}</td>
                  <td>{m.calls}</td>
                  <td>{fmtTokens(m.input_tokens)}</td>
                  <td>{fmtTokens(m.output_tokens)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
        <Card title={t('nodes.byProject')}>
          <table class="table">
            <thead>
              <tr>
                <th>{t('common.project')}</th>
                <th>Sessions</th>
                <th>{t('common.estimatedTraffic')}</th>
              </tr>
            </thead>
            <tbody>
              {(n.by_project || []).map((p: any) => (
                <tr key={p.project_id}>
                  <td>{p.project_id}</td>
                  <td>{p.sessions}</td>
                  <td>{fmtBytes(p.estimated_total_bytes)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      </div>

      <div class="grid-2">
        <Card title={t('overview.recentSessions')}>
          {(sessions.data?.sessions || []).map((s: any) => (
            <div key={s.id} class="list-row clickable" onClick={() => nav(`sessions/${s.id}`)}>
              <span>{s.title || s.client_id}</span>
              <span>{fmtDateTime(s.started_at)} · {s.model_call_count} calls</span>
            </div>
          ))}
        </Card>
        <Card title={t('overview.recentCalls')}>
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
