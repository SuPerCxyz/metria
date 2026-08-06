import { api, q } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { getRange, useQuery } from '../hooks/useQuery'
import { fmtBytes, fmtTokens, pct } from '../lib/format'
import { t } from '../lib/i18n'

export function DataQuality() {
  const range = getRange()
  const params = { from: range.from, to: range.to, timezone: range.timezone }
  const dq = useQuery<any>(`dq${q(params)}`, () => api(`/data-quality${q(params)}`))
  if (dq.error) return <ErrorBox error={dq.error} onRetry={dq.refresh} />
  if (dq.loading) return <Empty text={t('common.loading')} />
  const d = dq.data || {}

  const usage = (d.usage_distribution || []).filter((x: any) => x.usage_source)
  const traffic = (d.traffic_distribution || []).filter((x: any) => x.estimation_source)
  const usageTotal = usage.reduce((a: number, x: any) => a + x.calls, 0)
  const trafficTotal = traffic.reduce((a: number, x: any) => a + x.bytes, 0)

  return (
    <div class="page">
      <h2>{t('dataQuality.title')}</h2>
      <p class="page-note">帮助判断任意统计数字的可靠程度。</p>
      <div class="grid-2">
        <Card title={t('dataQuality.usageDist')}>
          {usage.length === 0 && <Empty text={t('common.empty')} />}
          <table class="table">
            <thead>
              <tr>
                <th>来源</th>
                <th>Calls</th>
                <th>占比</th>
              </tr>
            </thead>
            <tbody>
              {usage.map((x: any) => (
                <tr key={x.usage_source}>
                  <td>{x.usage_source}</td>
                  <td>{x.calls}</td>
                  <td>{pct(x.calls, usageTotal)}%</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
        <Card title={t('traffic.sourceDist')}>
          {traffic.length === 0 && <Empty text={t('common.empty')} />}
          <table class="table">
            <thead>
              <tr>
                <th>来源</th>
                <th>字节</th>
                <th>占比</th>
              </tr>
            </thead>
            <tbody>
              {traffic.map((x: any) => (
                <tr key={x.estimation_source}>
                  <td>{x.estimation_source}</td>
                  <td>{fmtBytes(x.bytes)}</td>
                  <td>{pct(x.bytes, trafficTotal)}%</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      </div>
      <Card title={t('overview.title')}>
        <table class="table">
          <tbody>
            <tr>
              <td>输入 Token（reported）</td>
              <td>{fmtTokens(usage.filter((x: any) => x.usage_source === 'reported').reduce((a: number, x: any) => a + x.tokens, 0))}</td>
            </tr>
            <tr>
              <td>解析告警数（source_errors）</td>
              <td>{d.parse_warnings ?? 0}</td>
            </tr>
          </tbody>
        </table>
      </Card>
    </div>
  )
}
