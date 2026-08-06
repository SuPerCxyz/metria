// 模型详情：摘要 + Token 趋势 + 费用趋势 + 使用模型的对象 + 缓存/延迟/错误率。

import React, { useMemo, useState } from 'react'
import { Link, useParams, useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DetailSummary from '../../components/common/DetailSummary'
import TrendChart from '../../components/charts/TrendChart'
import DataTable from '../../components/tables/DataTable'
import { ErrorState, LoadingSkeleton, EmptyState, DataQualityNote } from '../../components/feedback/Feedback'
import { api, q, rangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { fmtTokensShort, fmtUsd, fmtBytes, fmtDateTime } from '../../services/format'

const TREND_TABS = [
  { key: 'tokens', label: 'Token' },
  { key: 'cost', label: '费用' },
]

export default function ModelDetail() {
  const { id } = useParams()
  const navigate = useNavigate()
  const { range } = useTimeRange()
  const params = rangeParams(range)
  const [tab, setTab] = useState('tokens')

  const query = useQuery(`model-detail-${id}${q(params)}`, () => api(`/models/${encodeURIComponent(id)}${q(params)}`))

  const trend = useMemo(() => {
    const pts = query.data?.series || []
    if (tab === 'cost') return { labels: pts.map((p) => p.bucket), values: pts.map((p) => p.cost_micro_usd) }
    return { labels: pts.map((p) => p.bucket), values: pts.map((p) => p.input_tokens + p.output_tokens) }
  }, [query.data, tab])

  if (query.error) return <ErrorState error={query.error} />
  if (query.loading) return <LoadingSkeleton rows={8} />
  const d = query.data || {}
  const s = d.summary || {}
  const hasPricing = (d.pricing_rules || []).length > 0

  const recentColumns = [
    { key: 'started_at', label: '时间', render: (r) => fmtDateTime(r.started_at) },
    { key: 'model', label: '模型', render: (r) => r.model || '—' },
    { key: 'client_id', label: 'Agent', render: (r) => r.client_id || '—' },
    { key: 'input_tokens', label: 'Token', render: (r) => fmtTokensShort((r.input_tokens ?? 0) + (r.output_tokens ?? 0)) },
    { key: 'estimated_total_bytes', label: '流量', render: (r) => fmtBytes(r.estimated_total_bytes) },
  ]

  return (
    <>
      <PageHeader
        back={<Link to="/models" className="text-sm text-indigo-600 dark:text-indigo-400 hover:underline">← 返回模型</Link>}
        title={d.model || id}
        subtitle={d.provider || undefined}
      />

      {!hasPricing && <DataQualityNote kind="missing" text="该模型价格未配置，费用无法精确计算。" />}

      <div className="mt-4">
        <DetailSummary
          items={[
            { label: '请求数', value: String(s.model_calls ?? 0) },
            { label: 'Token', value: fmtTokensShort((s.input_tokens ?? 0) + (s.output_tokens ?? 0)) },
            { label: '费用', value: fmtUsd(s.cost_micro_usd) },
            { label: '网络流量', value: fmtBytes(s.estimated_total_bytes) },
            { label: '缓存命中率', value: (d.summary && s.input_tokens > 0) ? `${((s.cache_read_tokens ?? 0) / ((s.input_tokens ?? 0) + (s.cache_read_tokens ?? 0)) * 100).toFixed(1)}%` : '—' },
          ]}
        />
      </div>

      <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <div className="flex items-center justify-between mb-4 flex-wrap gap-3">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">Token / 费用趋势</h2>
          <div className="inline-flex rounded-lg bg-gray-100 dark:bg-gray-700/40 p-0.5">
            {TREND_TABS.map((t) => (
              <button key={t.key} type="button" onClick={() => setTab(t.key)} className={`px-3 py-1.5 text-sm font-medium rounded-md ${tab === t.key ? 'bg-white dark:bg-gray-600 shadow-xs text-gray-800 dark:text-gray-100' : 'text-gray-500 dark:text-gray-400'}`}>
                {t.label}
              </button>
            ))}
          </div>
        </div>
        {trend.labels.length === 0 ? <EmptyState title="当前范围无数据" /> : <TrendChart labels={trend.labels} values={trend.values} height={320} formatY={tab === 'cost' ? fmtUsd : fmtTokensShort} />}
      </div>

      <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-4">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 p-2">最近会话</h2>
        <DataTable columns={recentColumns} data={d.recent_sessions || []} pageSize={12} onRowClick={(r) => navigate(`/sessions/${encodeURIComponent(r.id)}`)} />
      </div>
    </>
  )
}
