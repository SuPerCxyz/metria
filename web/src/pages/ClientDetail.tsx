import { api, q } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { getRange, useQuery } from '../hooks/useQuery'
import { fmtBytes, fmtTokens, fmtUsd } from '../lib/format'
import { t } from '../lib/i18n'
import { nav } from '../lib/router'

/** Agent Tools（Client）详情：Node 分布、模型、最近会话。 */
export function ClientDetail({ id }: { id: string }) {
  const range = getRange()
  const params = { from: range.from, to: range.to, timezone: range.timezone }
  const detail = useQuery<any>(`client${id}${q(params)}`, () =>
    api(`/clients/${encodeURIComponent(id)}${q(params)}`),
  )
  const models = useQuery<any>(`client-models${id}`, () =>
    api(`/clients/${encodeURIComponent(id)}/models`),
  )

  if (detail.error) return <ErrorBox error={detail.error} onRetry={detail.refresh} />
  if (detail.loading) return <Empty text={t('common.loading')} />
  const d = detail.data || {}

  return (
    <div class="page">
      <button type="button" class="btn" onClick={() => nav('clients')}>
        ← {t('nav.clients')}
      </button>
      <h2>{d.client_id}</h2>
      <div class="stat-grid">
        {[
          [t('common.cost'), fmtUsd(d.calculated_cost_micro_usd)],
          [t('common.estimatedTraffic'), fmtBytes(d.estimated_total_bytes)],
        ].map(([label, value]) => (
          <div class="kv">
            <span class="kv-label">{label}</span>
            <span class="kv-value">{value}</span>
          </div>
        ))}
        {d.source_health && (
          <div class="kv">
            <span class="kv-label">{t('clients.sourceHealth')}</span>
            <span class="kv-value">
              {d.source_health.total ?? 0} / {d.source_health.healthy ?? 0}{' '}
              <span class="text-muted">
                {t('clients.healthy')}
                {(d.source_health.with_errors || 0) > 0 && (
                  <span> · {d.source_health.with_errors} {t('clients.withErrors')}</span>
                )}
              </span>
            </span>
          </div>
        )}
      </div>

      <div class="grid-2">
        <Card title={t('clients.byNode')}>
          <table class="table">
            <thead>
              <tr>
                <th>Node</th>
                <th>Input</th>
                <th>Output</th>
                <th>{t('common.estimatedTraffic')}</th>
                <th>Calls</th>
                <th>Sessions</th>
              </tr>
            </thead>
            <tbody>
              {(d.by_node || []).map((n: any) => (
                <tr key={n.node_id} class="clickable" onClick={() => nav(`nodes/${n.node_id}`)}>
                  <td>{n.node_id}</td>
                  <td>{fmtTokens(n.input_tokens)}</td>
                  <td>{fmtTokens(n.output_tokens)}</td>
                  <td>{fmtBytes(n.estimated_traffic_bytes)}</td>
                  <td>{n.model_calls}</td>
                  <td>{n.sessions}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>

        <Card title={t('clients.models')}>
          <table class="table">
            <thead>
              <tr>
                <th>{t('common.model')}</th>
                <th>Provider</th>
                <th>Calls</th>
                <th>Input</th>
                <th>Output</th>
              </tr>
            </thead>
            <tbody>
              {(models.data?.models || []).map((m: any) => (
                <tr key={`${m.model}-${m.provider || ''}`}>
                  <td>{m.model}</td>
                  <td>{m.provider || '—'}</td>
                  <td>{m.calls}</td>
                  <td>{fmtTokens(m.input_tokens)}</td>
                  <td>{fmtTokens(m.output_tokens)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>

        <Card title={t('clients.byProject')}>
          <table class="table">
            <thead>
              <tr>
                <th>{t('common.project')}</th>
                <th>Sessions</th>
                <th>Calls</th>
                <th>Input</th>
                <th>{t('common.estimatedTraffic')}</th>
              </tr>
            </thead>
            <tbody>
              {(d.by_project || []).map((p: any) => (
                <tr key={p.project_id}>
                  <td>{p.project_id}</td>
                  <td>{p.sessions}</td>
                  <td>{p.model_calls}</td>
                  <td>{fmtTokens(p.input_tokens)}</td>
                  <td>{fmtBytes(p.estimated_total_bytes)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>

        <Card title={t('clients.versionDist')}>
          <table class="table">
            <thead>
              <tr>
                <th>{t('common.version')}</th>
                <th>{t('common.count')}</th>
              </tr>
            </thead>
            <tbody>
              {(d.version_dist || []).map((v: any) => (
                <tr key={v.version}>
                  <td>{v.version}</td>
                  <td>{v.count}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      </div>

      <Card title={t('clients.recentSessions')}>
        <table class="table">
          <thead>
            <tr>
              <th>{t('sessions.titleColumn')}</th>
              <th>Node</th>
              <th>{t('common.model')}</th>
              <th>{t('common.startTime')}</th>
              <th>Calls</th>
              <th>{t('common.estimatedTraffic')}</th>
            </tr>
          </thead>
          <tbody>
            {(d.recent_sessions || []).map((s: any) => (
              <tr key={s.id} class="clickable" onClick={() => nav(`sessions/${s.id}`)}>
                <td>{s.title || s.source_session_id}</td>
                <td>{s.node_id}</td>
                <td>{s.model || '—'}</td>
                <td>{s.started_at}</td>
                <td>{s.model_call_count}</td>
                <td>{fmtBytes(s.estimated_total_bytes)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>

      <Card title={t('clients.recentCalls')}>
        <table class="table">
          <thead>
            <tr>
              <th>{t('common.model')}</th>
              <th>Provider</th>
              <th>{t('common.startTime')}</th>
              <th>Input</th>
              <th>Output</th>
              <th>{t('common.cost')}</th>
            </tr>
          </thead>
          <tbody>
            {(d.recent_calls || []).map((c: any) => (
              <tr key={c.id} class="clickable" onClick={() => nav(`calls/${c.id}`)}>
                <td>{c.model || '—'}</td>
                <td>{c.provider || '—'}</td>
                <td>{c.started_at}</td>
                <td>{fmtTokens(c.input_tokens)}</td>
                <td>{fmtTokens(c.output_tokens)}</td>
                <td>{fmtUsd(c.calculated_cost_micro_usd)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  )
}
