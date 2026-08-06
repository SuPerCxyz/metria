import { useEffect, useState } from 'preact/hooks'
import { API } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { fmtBytes, fmtDateTime, fmtUsd } from '../lib/format'
import { t } from '../lib/i18n'

/** 公开只读分享页：/s/{slug}，无需登录。 */
export function ShareView({ slug }: { slug: string }) {
  const [data, setData] = useState<any>(null)
  const [error, setError] = useState<string>('')
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    setLoading(true)
    setError('')
    fetch(`${API}/share/${slug}`)
      .then(async (res) => {
        if (!res.ok) {
          const body = await res.json().catch(() => null)
          throw new Error(body?.message || body?.error || `HTTP ${res.status}`)
        }
        return res.json()
      })
      .then(setData)
      .catch((e: Error) => setError(e.message))
      .finally(() => setLoading(false))
  }, [slug])

  if (loading) return <div class="state-box">{t('common.loading')}</div>
  if (error) return <ErrorBox error={error} />

  return (
    <div class="page">
      <h2>{data.kind === 'node' ? t('nav.nodes') : t('nav.sessions')}</h2>
      <p class="page-note">{t('shares.publicNote')}</p>
      {data.kind === 'session' ? <SessionView d={data} /> : <NodeView d={data} />}
    </div>
  )
}

function SessionView({ d }: { d: any }) {
  const s = d.session || {}
  const calls = d.calls || []
  return (
    <>
      <Card title={s.title || s.source_session_id || d.target_id}>
        <div class="stat-grid">
          <Stat label={t('shares.client')} value={s.client_id || '—'} />
          <Stat label={t('shares.messages')} value={s.message_count} />
          <Stat label={t('shares.toolCalls')} value={s.tool_call_count} />
          <Stat label={t('shares.modelCalls')} value={s.model_call_count} />
          <Stat label={t('shares.inputTokens')} value={s.input_tokens} />
          <Stat label={t('shares.outputTokens')} value={s.output_tokens} />
          <Stat label={t('shares.cost')} value={fmtUsd(s.estimated_cost_micro_usd)} />
          <Stat label={t('shares.traffic')} value={fmtBytes(s.estimated_total_bytes)} />
        </div>
        <p class="text-muted">{t('shares.startedAt')}: {fmtDateTime(s.started_at)}</p>
      </Card>
      {calls.length === 0 ? (
        <Empty text={t('shares.emptyCalls')} />
      ) : (
        <Card title={t('shares.calls')}>
          <table class="table">
            <thead>
              <tr>
                <th>{t('shares.model')}</th>
                <th>{t('shares.inputTokens')}</th>
                <th>{t('shares.outputTokens')}</th>
                <th>{t('shares.traffic')}</th>
              </tr>
            </thead>
            <tbody>
              {calls.map((c: any) => (
                <tr key={c.id}>
                  <td>{c.model || '—'}</td>
                  <td>{c.input_tokens ?? '—'}</td>
                  <td>{c.output_tokens ?? '—'}</td>
                  <td>{fmtBytes(c.estimated_total_bytes)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}
    </>
  )
}

function NodeView({ d }: { d: any }) {
  const n = d.node || {}
  const clients = d.clients || []
  return (
    <>
      <Card title={n.name || d.target_id}>
        <div class="stat-grid">
          <Stat label={t('shares.platform')} value={n.platform || '—'} />
          <Stat label={t('shares.status')} value={n.status || '—'} />
          <Stat label={t('shares.firstSeen')} value={fmtDateTime(n.first_seen_at)} />
          <Stat label={t('shares.lastSeen')} value={fmtDateTime(n.last_seen_at)} />
        </div>
      </Card>
      <Card title={t('shares.clients')}>
        {clients.length === 0 ? (
          <Empty text={t('shares.emptyClients')} />
        ) : (
          <table class="table">
            <thead>
              <tr>
                <th>{t('shares.client')}</th>
                <th>{t('shares.sessions')}</th>
                <th>{t('shares.modelCalls')}</th>
              </tr>
            </thead>
            <tbody>
              {clients.map((c: any) => (
                <tr key={c.client_id}>
                  <td>{c.client_id}</td>
                  <td>{c.session_count ?? '—'}</td>
                  <td>{c.model_call_count ?? '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
    </>
  )
}

function Stat({ label, value }: { label: string; value: any }) {
  return (
    <div class="stat">
      <div class="stat-label">{label}</div>
      <div class="stat-value">{value === null || value === undefined || value === '' ? '—' : String(value)}</div>
    </div>
  )
}
