// 使用分析：Token / 请求 / 缓存 / 延迟 标签切换。

import React, { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import MetricCard from '../../components/cards/MetricCard'
import TrendChart from '../../components/charts/TrendChart'
import RankingList from '../../components/cards/RankingList'
import { ErrorState, LoadingSkeleton, EmptyState } from '../../components/feedback/Feedback'
import { api, q, rangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { fmtTokensShort, fmtUsd, fmtBytes, fmtPct100 } from '../../services/format'

const TABS = [
  { key: 'tokens', label: 'Token' },
  { key: 'requests', label: '请求' },
  { key: 'cache', label: '缓存' },
  { key: 'latency', label: '延迟' },
]

export default function Analytics() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const params = rangeParams(range)
  const [tab, setTab] = useState('tokens')
  const [trendTab, setTrendTab] = useState('tokens')

  const overview = useQuery(`overview${q(params)}`, () => api(`/overview${q(params)}`))
  const series = useQuery(`ts${q(params)}`, () => api(`/usage/timeseries${q(params)}`))
  const byModel = useQuery(`breakdown${q({ ...params, dim: 'model' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'model' })}`))
  const byAgent = useQuery(`bclient${q({ ...params, dim: 'client' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'client' })}`))
  const byNode = useQuery(`bnode${q({ ...params, dim: 'node' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'node' })}`))

  const trendData = useMemo(() => {
    const pts = series.data?.series || []
    switch (trendTab) {
      case 'tokens': return { labels: pts.map((p) => p.bucket), values: pts.map((p) => p.input_tokens + p.output_tokens) }
      case 'cost': return { labels: pts.map((p) => p.bucket), values: pts.map((p) => p.cost_micro_usd) }
      case 'traffic': return { labels: pts.map((p) => p.bucket), values: pts.map((p) => p.estimated_traffic_bytes) }
      default: return { labels: pts.map((p) => p.bucket), values: pts.map((p) => p.model_calls) }
    }
  }, [series.data, trendTab])

  if (overview.error) return <ErrorState error={overview.error} onRetry={overview.refresh} />
  if (overview.loading) return <LoadingSkeleton rows={6} />
  const o = overview.data || {}

  const cacheHitRate = o.input_tokens > 0 ? o.cache_read_tokens / (o.input_tokens + o.cache_read_tokens) : 0

  const modelItems = (byModel.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '' && m.dimension !== '(unknown)')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: m.input_tokens + m.output_tokens }))
  const agentItems = (byAgent.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: m.input_tokens + m.output_tokens }))
  const nodeItems = (byNode.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: m.input_tokens + m.output_tokens }))

  return (
    <>
      <PageHeader title="使用分析" subtitle="深入分析 Token、请求、缓存与延迟" />
      <div className="inline-flex rounded-lg bg-gray-100 dark:bg-gray-700/40 p-1 mb-6">
        {TABS.map((t) => (
          <button
            key={t.key}
            type="button"
            onClick={() => setTab(t.key)}
            className={`px-5 py-2.5 text-base font-medium rounded-md ${tab === t.key ? 'bg-white dark:bg-gray-600 shadow-xs text-gray-800 dark:text-gray-100' : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200'}`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {tab === 'tokens' && (
        <>
          <div className="grid grid-cols-12 gap-6">
            <MetricCard label="总 Token" value={fmtTokensShort((o.input_tokens ?? 0) + (o.output_tokens ?? 0) + (o.cache_read_tokens ?? 0))} sub={`输入 ${fmtTokensShort(o.input_tokens)} · 输出 ${fmtTokensShort(o.output_tokens)}`} />
            <MetricCard label="输入 Token" value={fmtTokensShort(o.input_tokens)} sub="请求上下文" />
            <MetricCard label="输出 Token" value={fmtTokensShort(o.output_tokens)} sub="模型生成" />
            <MetricCard label="缓存 Token" value={fmtTokensShort(o.cache_read_tokens)} sub="缓存读取" />
          </div>
          <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
            <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">Token 趋势</h2>
            <TrendChart labels={trendData.labels} values={trendData.values} height={340} formatY={fmtTokensShort} />
          </div>
          <div className="mt-6 grid grid-cols-1 xl:grid-cols-3 gap-6">
            <RankingCard title="Agent Token 排行" items={agentItems} onClick={(i) => navigate(`/agents/${encodeURIComponent(i.id)}`)} />
            <RankingCard title="模型 Token 排行" items={modelItems} onClick={(i) => navigate(`/models/${encodeURIComponent(i.id)}`)} />
            <RankingCard title="节点 Token 排行" items={nodeItems} onClick={(i) => navigate(`/nodes/${encodeURIComponent(i.id)}`)} />
          </div>
        </>
      )}

      {tab === 'requests' && (
        <>
          <div className="grid grid-cols-12 gap-6">
            <MetricCard label="请求总数" value={fmtTokensShort(o.model_calls)} />
            <MetricCard label="成功请求" value={fmtTokensShort(o.model_calls)} sub="估算" />
            <MetricCard label="失败请求" value={0} sub="暂无错误数据" />
            <MetricCard label="平均请求频率" value={o.sessions > 0 ? ((o.model_calls ?? 0) / o.sessions).toFixed(1) : '—'} sub="每会话请求数" />
          </div>
          <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
            <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">请求趋势</h2>
            <TrendChart labels={trendData.labels} values={trendData.values} height={340} formatY={(v) => v.toLocaleString()} />
          </div>
        </>
      )}

      {tab === 'cache' && (
        <>
          <div className="grid grid-cols-12 gap-6">
            <MetricCard label="缓存 Token" value={fmtTokensShort(o.cache_read_tokens)} sub="缓存读取" />
            <MetricCard label="缓存命中率" value={fmtPct100(cacheHitRate)} />
            <MetricCard label="缓存节省费用" value="—" sub="暂无价格数据" />
            <MetricCard label="缓存写入" value={fmtTokensShort(o.cache_write_tokens)} />
          </div>
          <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
            <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">缓存趋势</h2>
            <TrendChart labels={trendData.labels} values={trendData.values} height={340} formatY={fmtTokensShort} />
          </div>
        </>
      )}

      {tab === 'latency' && (
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-2">延迟分析</h2>
          <EmptyState title="暂无延迟数据" desc="后端暂未提供延迟指标（P50/P95/P99），后续版本补充。" />
        </div>
      )}
    </>
  )
}

function RankingCard({ title, items, onClick }) {
  const count = (items || []).length
  return (
    <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">{title}</h2>
        <span className="text-xs text-gray-400 dark:text-gray-500">共 {count} 项</span>
      </div>
      <RankingList items={items} valueKey="value" labelKey="name" format={fmtTokensShort} limit={count || 1} onItemClick={onClick} />
    </div>
  )
}
