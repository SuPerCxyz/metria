import { api, q } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { getRange, useQuery } from '../hooks/useQuery'
import { fmtBytes, fmtTokens, fmtUsd } from '../lib/format'
import { t } from '../lib/i18n'
import { nav } from '../lib/router'

export function Models() {
  const range = getRange()
  const params = { from: range.from, to: range.to, timezone: range.timezone }
  const models = useQuery<any>(`models${q(params)}`, () => api(`/models${q(params)}`))
  if (models.error) return <ErrorBox error={models.error} onRetry={models.refresh} />
  if (models.loading) return <Empty text={t('common.loading')} />

  return (
    <div class="page">
      <h2>{t('nav.models')}</h2>
      <Card>
        <table class="table">
          <thead>
            <tr>
              <th>{t('common.model')}</th>
              <th>Provider</th>
              <th>Input</th>
              <th>Output</th>
              <th>{t('common.estimatedTraffic')}</th>
              <th>Calls</th>
              <th>Sessions</th>
              <th>Clients</th>
              <th>Nodes</th>
              <th>{t('models.pricingSource')}</th>
            </tr>
          </thead>
          <tbody>
            {(models.data?.models || []).map((m: any) => (
              <tr key={m.model} class="clickable" onClick={() => nav(`models/${encodeURIComponent(m.model)}`)}>
                <td>{m.model}</td>
                <td>{m.provider || '—'}</td>
                <td>{fmtTokens(m.input_tokens)}</td>
                <td>{fmtTokens(m.output_tokens)}</td>
                <td>{fmtBytes(m.estimated_traffic_bytes)}</td>
                <td>{m.model_calls}</td>
                <td>{m.sessions}</td>
                <td>{m.clients}</td>
                <td>{m.nodes}</td>
                <td class="mono">{m.pricing_source || 'builtin_catalog'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  )
}

/** Models Detail：汇总、Pricing 规则、最近会话（S3.6）。 */
export function ModelDetail({ id }: { id: string }) {
  const range = getRange()
  const params = { from: range.from, to: range.to, timezone: range.timezone }
  const detail = useQuery<any>(`model${id}${q(params)}`, () =>
    api(`/models/${encodeURIComponent(id)}${q(params)}`),
  )
  if (detail.error) return <ErrorBox error={detail.error} onRetry={detail.refresh} />
  if (detail.loading) return <Empty text={t('common.loading')} />
  const d = detail.data || {}
  const s = d.summary || {}

  return (
    <div class="page">
      <button type="button" class="btn" onClick={() => nav('models')}>
        ← {t('nav.models')}
      </button>
      <h2>{d.model}</h2>
      <div class="stat-grid">
        {[
          [t('common.input'), fmtTokens(s.input_tokens)],
          [t('common.output'), fmtTokens(s.output_tokens)],
          [t('common.cacheRead'), fmtTokens(s.cache_read_tokens)],
          [t('common.cacheWrite'), fmtTokens(s.cache_write_tokens)],
          [t('common.reasoning'), fmtTokens(s.reasoning_tokens)],
          [t('common.cost'), fmtUsd(s.cost_micro_usd)],
          [t('common.estimatedTraffic'), fmtBytes(s.estimated_total_bytes)],
          ['Calls', String(s.model_calls ?? 0)],
          ['Sessions', String(s.sessions ?? 0)],
        ].map(([label, value]) => (
          <div class="kv">
            <span class="kv-label">{label}</span>
            <span class="kv-value">{value}</span>
          </div>
        ))}
      </div>

      <div class="grid-2">
        <Card title={t('models.rawNames')}>
          <table class="table">
            <thead>
              <tr>
                <th>Raw Model</th>
                <th>Provider</th>
                <th>Calls</th>
              </tr>
            </thead>
            <tbody>
              {(d.raw_names || []).map((r: any) => (
                <tr key={`${r.model_raw}-${r.provider || ''}`}>
                  <td>{r.model_raw || '—'}</td>
                  <td>{r.provider || '—'}</td>
                  <td>{r.calls}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>

        <Card title={t('models.pricingRules')}>
          <table class="table">
            <thead>
              <tr>
                <th>{t('common.model')}</th>
                <th>Provider</th>
                <th>Input/百万</th>
                <th>Output/百万</th>
                <th>Source</th>
              </tr>
            </thead>
            <tbody>
              {(d.pricing_rules || []).map((r: any) => (
                <tr key={r.id}>
                  <td>{r.model_pattern}</td>
                  <td>{r.provider_pattern}</td>
                  <td>{r.input_price != null ? fmtUsd(r.input_price) : '—'}</td>
                  <td>{r.output_price != null ? fmtUsd(r.output_price) : '—'}</td>
                  <td class="mono">{r.source}</td>
                </tr>
              ))}
              {(d.pricing_rules || []).length === 0 && (
                <tr>
                  <td colSpan={5}>{t('common.empty')}</td>
                </tr>
              )}
            </tbody>
          </table>
        </Card>
      </div>

      <Card title={t('models.recentSessions')}>
        <table class="table">
          <thead>
            <tr>
              <th>{t('sessions.titleColumn')}</th>
              <th>{t('sessions.client')}</th>
              <th>{t('common.startTime')}</th>
              <th>Calls</th>
              <th>{t('common.estimatedTraffic')}</th>
            </tr>
          </thead>
          <tbody>
            {(d.recent_sessions || []).map((sd: any) => (
              <tr key={sd.id} class="clickable" onClick={() => nav(`sessions/${sd.id}`)}>
                <td>{sd.title || sd.source_session_id}</td>
                <td>{sd.client_id}</td>
                <td>{sd.started_at}</td>
                <td>{sd.model_call_count}</td>
                <td>{fmtBytes(sd.estimated_total_bytes)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  )
}
