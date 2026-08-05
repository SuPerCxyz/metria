import { api, q } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { getRange, useQuery } from '../hooks/useQuery'
import { fmtBytes, fmtTokens } from '../lib/format'

export function Models() {
  const range = getRange()
  const params = { from: range.from, to: range.to, timezone: range.timezone }
  const models = useQuery<any>(`models${q(params)}`, () => api(`/models${q(params)}`))
  if (models.error) return <ErrorBox error={models.error} onRetry={models.refresh} />
  if (models.loading) return <Empty text="加载中…" />

  return (
    <div class="page">
      <h2>模型</h2>
      <Card>
        <table class="table">
          <thead>
            <tr>
              <th>模型</th>
              <th>Provider</th>
              <th>Input</th>
              <th>Output</th>
              <th>估算流量</th>
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
