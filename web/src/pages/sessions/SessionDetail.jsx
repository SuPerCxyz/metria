// 会话详情：摘要 + 趋势图 + Token 构成 + 模型调用列表 + 错误异常。

import React, { useMemo, useState } from 'react'
import { Link, useParams, useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DetailSummary from '../../components/common/DetailSummary'
import StatusBadge from '../../components/common/StatusBadge'
import DataTable from '../../components/tables/DataTable'
import TrendChart from '../../components/charts/TrendChart'
import { ErrorState, LoadingSkeleton, EmptyState } from '../../components/feedback/Feedback'
import { api } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { fmtTokensShort, fmtUsd, fmtBytes, fmtDateTime, fmtDuration } from '../../services/format'

const TREND_TABS = [
  { key: 'tokens', label: 'Token' },
  { key: 'cost', label: '费用' },
  { key: 'traffic', label: '流量' },
  { key: 'latency', label: '延迟' },
]

export default function SessionDetail() {
  const { id } = useParams()
  const navigate = useNavigate()
  const [trendTab, setTrendTab] = useState('tokens')

  const query = useQuery(`session-detail-${id}`, () => api(`/sessions/${encodeURIComponent(id)}`))
  const calls = useQuery(`session-calls-${id}`, () => api(`/sessions/${encodeURIComponent(id)}/calls`))

  const callList = calls.data?.calls || []

  const trendData = useMemo(() => {
    const pts = callList
    switch (trendTab) {
      case 'tokens': return { labels: pts.map((p) => p.started_at), values: pts.map((p) => (p.input_tokens ?? 0) + (p.output_tokens ?? 0)) }
      case 'cost': return { labels: pts.map((p) => p.started_at), values: pts.map((p) => p.calculated_cost_micro_usd ?? 0) }
      case 'traffic': return { labels: pts.map((p) => p.started_at), values: pts.map((p) => p.estimated_total_bytes ?? 0) }
      case 'latency': return { labels: pts.map((p) => p.started_at), values: pts.map((p) => p.duration_ms ?? 0) }
      default: return { labels: [], values: [] }
    }
  }, [callList, trendTab])

  const formatY = (v) => {
    if (trendTab === 'tokens') return fmtTokensShort(v)
    if (trendTab === 'cost') return fmtUsd(v)
    if (trendTab === 'traffic') return fmtBytes(v)
    return `${v}ms`
  }

  const callColumns = [
    { key: 'started_at', label: '调用时间', sortable: true, render: (r) => fmtDateTime(r.started_at) },
    { key: 'model', label: '模型', render: (r) => r.model || '—' },
    { key: 'input_tokens', label: '输入 Token', render: (r) => fmtTokensShort(r.input_tokens) },
    { key: 'output_tokens', label: '输出 Token', render: (r) => fmtTokensShort(r.output_tokens) },
    { key: 'cache_read_tokens', label: '缓存 Token', render: (r) => fmtTokensShort(r.cache_read_tokens) },
    { key: 'calculated_cost_micro_usd', label: '费用', render: (r) => fmtUsd(r.calculated_cost_micro_usd) },
    { key: 'duration_ms', label: '响应时间', render: (r) => fmtDuration(r.duration_ms) },
    { key: 'status', label: '状态', render: (r) => <StatusBadge status={r.status} /> },
  ]

  const errors = (callList || []).filter((c) => c.status && !['success', 'ok', 'completed'].includes(String(c.status).toLowerCase()))

  if (query.error) return <ErrorState error={query.error} />
  if (query.loading) return <LoadingSkeleton rows={8} />
  const s = query.data?.session || {}

  return (
    <>
      <PageHeader
        back={<Link to="/sessions" className="text-sm text-indigo-600 dark:text-indigo-400 hover:underline">← 返回会话</Link>}
        title={s.title || s.source_session_id || id}
        subtitle={<span>{s.client_id} · {s.node_id} · <StatusBadge status={s.status} /></span>}
      />

      <DetailSummary
        items={[
          { label: 'Agent', value: s.client_id || '—' },
          { label: '节点', value: s.node_id || '—' },
          { label: '开始时间', value: fmtDateTime(s.started_at) },
          { label: '结束时间', value: s.ended_at ? fmtDateTime(s.ended_at) : '—' },
          { label: '持续时间', value: fmtDuration((new Date(s.ended_at || Date.now()) - new Date(s.started_at)).valueOf()) },
          { label: '调用次数', value: String(s.model_call_count ?? 0) },
          { label: '总 Token', value: fmtTokensShort((s.input_tokens ?? 0) + (s.output_tokens ?? 0)) },
          { label: '总费用', value: fmtUsd(s.calculated_cost_micro_usd ?? s.estimated_cost_micro_usd) },
          { label: '总流量', value: fmtBytes(s.estimated_total_bytes) },
          { label: '模型', value: s.model || '—' },
        ]}
      />

      <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <div className="flex items-center justify-between mb-4 flex-wrap gap-3">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">会话趋势</h2>
          <div className="inline-flex rounded-lg bg-gray-100 dark:bg-gray-700/40 p-0.5">
            {TREND_TABS.map((t) => (
              <button key={t.key} type="button" onClick={() => setTrendTab(t.key)} className={`px-3 py-1.5 text-sm font-medium rounded-md ${trendTab === t.key ? 'bg-white dark:bg-gray-600 shadow-xs text-gray-800 dark:text-gray-100' : 'text-gray-500 dark:text-gray-400'}`}>
                {t.label}
              </button>
            ))}
          </div>
        </div>
        {trendData.labels.length === 0 ? <EmptyState title="该会话暂无模型调用" /> : <TrendChart labels={trendData.labels} values={trendData.values} height={320} formatY={formatY} />}
      </div>

      <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">Token 构成</h2>
        <div className="flex flex-wrap gap-6">
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-bold text-gray-800 dark:text-gray-100 tabular-nums">{fmtTokensShort(s.input_tokens)}</span>
            <span className="text-sm text-gray-500 dark:text-gray-400">输入</span>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-bold text-gray-800 dark:text-gray-100 tabular-nums">{fmtTokensShort(s.output_tokens)}</span>
            <span className="text-sm text-gray-500 dark:text-gray-400">输出</span>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-bold text-gray-800 dark:text-gray-100 tabular-nums">{fmtTokensShort(s.cache_read_tokens)}</span>
            <span className="text-sm text-gray-500 dark:text-gray-400">缓存</span>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-bold text-gray-800 dark:text-gray-100 tabular-nums">{fmtTokensShort(s.reasoning_tokens)}</span>
            <span className="text-sm text-gray-500 dark:text-gray-400">推理</span>
          </div>
        </div>
      </div>

      <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-4">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 p-2">模型调用列表</h2>
        <DataTable columns={callColumns} data={callList} pageSize={15} onRowClick={(r) => navigate(`/calls/${encodeURIComponent(r.id)}`)} />
      </div>

      {errors.length > 0 && (
        <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-red-200 dark:border-red-800/60 p-6">
          <h2 className="text-lg font-bold text-red-600 dark:text-red-400 mb-4">错误和异常</h2>
          <div className="space-y-2">
            {errors.slice(0, 10).map((e) => (
              <div key={e.id} className="flex items-center justify-between px-3 py-2 rounded-lg bg-red-50 dark:bg-red-400/5 text-sm">
                <span className="text-gray-700 dark:text-gray-200">{e.model || '—'} · {fmtDateTime(e.started_at)}</span>
                <StatusBadge status={e.status} />
              </div>
            ))}
          </div>
        </div>
      )}
    </>
  )
}
