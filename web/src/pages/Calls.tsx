import { useState } from 'preact/hooks'
import { api, q } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { getRange, useQuery } from '../hooks/useQuery'
import { fmtBytes, fmtDateTime, fmtTokens, fmtUsd } from '../lib/format'
import { t } from '../lib/i18n'
import { nav } from '../lib/router'

type SortKey = 'started_at' | 'model' | 'client_id' | 'input_tokens' | 'output_tokens' | 'calculated_cost_micro_usd'

const SORTABLE: { key: SortKey; label: string }[] = [
  { key: 'started_at', label: '时间' },
  { key: 'client_id', label: 'Client' },
  { key: 'model', label: '模型' },
  { key: 'input_tokens', label: 'Input' },
  { key: 'output_tokens', label: 'Output' },
  { key: 'calculated_cost_micro_usd', label: '费用' },
]

function sortCalls(list: any[], key: SortKey, dir: 1 | -1): any[] {
  return [...list].sort((a, b) => {
    const av = a[key] ?? ''
    const bv = b[key] ?? ''
    const cmp = typeof av === 'number' ? av - (bv as number) : String(av).localeCompare(String(bv))
    return cmp * dir
  })
}

export function Calls() {
  const range = getRange()
  const params = { from: range.from, to: range.to, timezone: range.timezone }
  const [sortKey, setSortKey] = useState<SortKey>('started_at')
  const [sortDir, setSortDir] = useState<1 | -1>(-1)
  const [cursor, setCursor] = useState<string | undefined>()
  const [all] = useState<any[]>([])
  const [loadingMore, setLoadingMore] = useState(false)

  const pageParams = cursor ? { ...params, cursor } : params
  const calls = useQuery<any>(`calls${q({ ...pageParams, limit: 200 })}`, () =>
    api(`/calls${q({ ...pageParams, limit: 200 })}`),
  )

  if (calls.error) return <ErrorBox error={calls.error} onRetry={calls.refresh} />
  if (calls.loading && all.length === 0) return <Empty text={t('common.loading')} />

  const pageList: any[] = calls.data?.calls || []
  const list = cursor ? [...all, ...pageList] : pageList
  const nextCursor = calls.data?.next_cursor as string | undefined

  const toggleSort = (key: SortKey) => {
    if (key === sortKey) setSortDir((d) => (d === 1 ? -1 : 1))
    else {
      setSortKey(key)
      setSortDir(key === 'started_at' ? -1 : 1)
    }
  }

  const loadMore = async () => {
    if (!nextCursor || loadingMore) return
    setLoadingMore(true)
    setCursor(nextCursor)
    setLoadingMore(false)
  }

  const sorted = sortCalls(list, sortKey, sortDir)

  return (
    <div class="page">
      <h2>{t('calls.title')}</h2>
      <Card>
        <table class="table">
          <thead>
            <tr>
              {SORTABLE.map((col) => (
                <th
                  key={col.key}
                  class="clickable"
                  onClick={() => toggleSort(col.key)}
                  title={sortKey === col.key ? (sortDir === 1 ? '升序' : '降序') : '点击排序'}
                >
                  {col.label}
                  {sortKey === col.key ? (sortDir === 1 ? ' ▲' : ' ▼') : ''}
                </th>
              ))}
              <th>Provider</th>
              <th>{t('common.status')}</th>
              <th>Cache</th>
            </tr>
          </thead>
          <tbody>
            {sorted.map((c: any) => (
              <tr key={c.id} class="clickable" onClick={() => nav(`calls/${c.id}`)}>
                <td>{fmtDateTime(c.started_at)}</td>
                <td>{c.client_id}</td>
                <td>{c.model || '—'}</td>
                <td>{fmtTokens(c.input_tokens)}</td>
                <td>{fmtTokens(c.output_tokens)}</td>
                <td>{fmtUsd(c.calculated_cost_micro_usd ?? c.reported_cost_micro_usd)}</td>
                <td>{c.provider || '—'}</td>
                <td>{c.status}</td>
                <td>{fmtTokens((c.cache_read_tokens ?? 0) + (c.cache_write_tokens ?? 0))}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {nextCursor && (
          <div class="dim-switch" style="margin-top:10px">
            <button type="button" class="btn small" onClick={loadMore} disabled={loadingMore}>
              {t('common.more')} ↓
            </button>
          </div>
        )}
      </Card>
    </div>
  )
}

export function CallDetail({ id }: { id: string }) {
  const call = useQuery<any>(`call${id}`, () => api(`/calls/${encodeURIComponent(id)}`))
  if (call.error) return <ErrorBox error={call.error} onRetry={call.refresh} />
  if (call.loading) return <Empty text={t('common.loading')} />
  const c = call.data?.call || {}
  const tr = call.data?.traffic || {}

  return (
    <div class="page">
      <button type="button" class="btn" onClick={() => nav('calls')}>
        ← Calls
      </button>
      <h2>Call：{c.id}</h2>
      <div class="stat-grid">
        {[
          ['Client', c.client_id],
          ['Session', c.session_id || '—'],
          [t('common.model'), c.model_raw || c.model || '—'],
          ['Provider', c.provider_raw || c.provider || '—'],
          ['开始', fmtDateTime(c.started_at)],
          ['完成', c.completed_at ? fmtDateTime(c.completed_at) : '—'],
          [t('common.status'), c.status],
          ['粒度', c.call_granularity],
          ['Input', fmtTokens(c.input_tokens)],
          ['Output', fmtTokens(c.output_tokens)],
          ['Cache Read', fmtTokens(c.cache_read_tokens)],
          ['Reasoning', fmtTokens(c.reasoning_tokens)],
          ['Reported Cost', fmtUsd(c.reported_cost_micro_usd)],
          ['Calculated Cost', fmtUsd(c.calculated_cost_micro_usd)],
        ].map(([label, value]) => (
          <div class="kv" key={label}>
            <span class="kv-label">{label}</span>
            <span class="kv-value">{value}</span>
          </div>
        ))}
      </div>

      <Card title={t('common.estimatedTraffic')}>
        <div class="traffic-display">
          <div class="traffic-main">
            估算流量：{fmtBytes(tr.estimated_total_wire_bytes)}
            <span class="traffic-range">
              估算范围：{fmtBytes(tr.lower_bound_bytes)} ~ {fmtBytes(tr.upper_bound_bytes)}
            </span>
          </div>
          <div class="kv-grid">
            {[
              [t('traffic.request'), fmtBytes(tr.estimated_request_wire_bytes)],
              [t('traffic.response'), fmtBytes(tr.estimated_response_wire_bytes)],
              ['可信度', tr.confidence != null ? `${Math.round(tr.confidence * 100)}%` : '—'],
              ['估算来源', tr.estimation_source],
              ['上下文传输', tr.context_transport_mode],
              ['Cache 行为', tr.cache_transport_behavior],
            ].map(([label, value]) => (
              <div class="kv" key={label}>
                <span class="kv-label">{label}</span>
                <span class="kv-value">{value}</span>
              </div>
            ))}
          </div>
          <p class="traffic-note">
            提示：以上为根据客户端日志与 Token 估算的「估算流量」，不代表网卡真实流量或云厂商计费流量。
          </p>
        </div>
      </Card>
    </div>
  )
}
