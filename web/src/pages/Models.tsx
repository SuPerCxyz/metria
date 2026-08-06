import { api, q } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { getRange, useQuery } from '../hooks/useQuery'
import { fmtBytes, fmtTokens } from '../lib/format'
import { t } from '../lib/i18n'

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
            </tr>
          </thead>
          <tbody>
            {(models.data?.models || []).map((m: any) => (
              <tr key={m.model}>
                <td>{m.model}</td>
                <td>{m.provider || '—'}</td>
                <td>{fmtTokens(m.input_tokens)}</td>
                <td>{fmtTokens(m.output_tokens)}</td>
                <td>{fmtBytes(m.estimated_traffic_bytes)}</td>
                <td>{m.model_calls}</td>
                <td>{m.sessions}</td>
                <td>{m.clients}</td>
                <td>{m.nodes}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  )
}
