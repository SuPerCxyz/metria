import { api, q } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { getRange, useQuery } from '../hooks/useQuery'
import { fmtBytes, fmtTokens, fmtUsd } from '../lib/format'
import { t } from '../lib/i18n'
import { nav } from '../lib/router'

export function Clients() {
  const range = getRange()
  const params = { from: range.from, to: range.to, timezone: range.timezone }
  const clients = useQuery<any>(`clients${q(params)}`, () => api(`/clients${q(params)}`))
  if (clients.error) return <ErrorBox error={clients.error} onRetry={clients.refresh} />
  if (clients.loading) return <Empty text={t('common.loading')} />

  return (
    <div class="page">
      <h2>{t('nav.clients')}</h2>
      <Card>
        <table class="table">
          <thead>
            <tr>
              <th>Client</th>
              <th>Input</th>
              <th>Output</th>
              <th>{t('common.estimatedTraffic')}</th>
              <th>Calls</th>
              <th>Sessions</th>
              <th>{t('common.cost')}</th>
            </tr>
          </thead>
          <tbody>
            {(clients.data?.clients || []).map((c: any) => (
              <tr key={c.client_id} class="clickable" onClick={() => nav(`clients/${c.client_id}`)}>
                <td>{c.client_id}</td>
                <td>{fmtTokens(c.input_tokens)}</td>
                <td>{fmtTokens(c.output_tokens)}</td>
                <td>{fmtBytes(c.estimated_traffic_bytes)}</td>
                <td>{c.model_calls}</td>
                <td>{c.sessions}</td>
                <td>{fmtUsd(c.cost_micro_usd)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  )
}
