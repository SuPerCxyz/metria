// 总览页：4 核心指标 + 主趋势图 + 双排行 + 关注事件。

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
import { fmtTokensShort, fmtUsd, fmtBytes, fmtTokens } from '../../services/format'

const TREND_TABS = [
  { key: 'tokens', label: 'Token' },
  { key: 'cost', label: '费用' },
  { key: 'traffic', label: '流量' },
  { key: 'requests', label: '请求数' },
]

export default function Overview() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const params = rangeParams(range)
  const [trendTab, setTrendTab] = useState('tokens')
  const [costTab, setCostTab] = useState('cost')

  const overview = useQuery(`overview${q(params)}`, () => api(`/overview${q(params)}`))
  const series = useQuery(`ts${q(params)}`, () => api(`/usage/timeseries${q(params)}`))
  const byDim = useQuery(`breakdown-cost${q({ ...params, dim: 'model' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'model' })}`))
  const byAgent = useQuery(`breakdown-client${q({ ...params, dim: 'client' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'client' })}`))

  const trendData = useMemo(() => {
    const pts = series.data?.series || []
    switch (trendTab) {
      case 'tokens': return { labels: pts.map((p) => p.bucket), values: pts.map((p) => p.input_tokens + p.output_tokens) }
      case 'cost': return { labels: pts.map((p) => p.bucket), values: pts.map((p) => p.cost_micro_usd) }
      case 'traffic': return { labels: pts.map((p) => p.bucket), values: pts.map((p) => p.estimated_traffic_bytes) }
      case 'requests': return { labels: pts.map((p) => p.bucket), values: pts.map((p) => p.model_calls) }
      default: return { labels: [], values: [] }
    }
  }, [series.data, trendTab])

  const formatY = (v) => {
    if (trendTab === 'tokens') return fmtTokensShort(v)
    if (trendTab === 'cost') return fmtUsd(v)
    if (trendTab === 'traffic') return fmtBytes(v)
    return v.toLocaleString()
  }

  // 关注事件：从已有数据推导
  const alerts = useMemo(() => {
    const list = []
    const o = overview.data || {}
    const topCalls = (byDim.data?.by || []).slice(0, 5)
    for (const m of topCalls) {
      if (m.model_calls > 0) list.push({ type: 'cost', text: `${m.dimension} 模型调用 ${m.model_calls} 次`, ts: null })
    }
    return list.slice(0, 5)
  }, [overview.data, byDim.data])

  if (overview.error) return <ErrorState error={overview.error} onRetry={overview.refresh} />
  if (overview.loading) return <LoadingSkeleton rows={6} />
  const o = overview.data || {}

  const costItems = (byDim.data?.by || []).map((m) => ({ id: m.dimension, name: m.dimension, value: m.model_calls }))
  const agentItems = (byAgent.data?.by || []).map((m) => ({ id: m.dimension, name: m.dimension, value: m.model_calls }))

  return (
    <>
      <PageHeader title="总览" subtitle="AI 编程 Agent 用量、费用与流量概览" />

      {/* 第一行：4 核心指标 */}
      <div className="grid grid-cols-12 gap-6">
        <MetricCard
          label="总费用"
          value={fmtUsd(o.calculated_cost_micro_usd ?? o.estimated_cost_micro_usd)}
          sub="估算费用"
          hint="按 Token 与价格目录估算"
        />
        <MetricCard
          label="总 Token"
          value={fmtTokens((o.input_tokens ?? 0) + (o.output_tokens ?? 0))}
          sub={
            <span className="tabular-nums">
              <span className="text-gray-400 dark:text-gray-500">输入 {fmtTokensShort(o.input_tokens)} · 输出 {fmtTokensShort(o.output_tokens)} · 缓存 {fmtTokensShort(o.cache_read_tokens)}</span>
            </span>
          }
        />
        <MetricCard
          label="网络流量"
          value={fmtBytes(o.estimated_total_bytes)}
          sub="估算流量（含上下界）"
          hint={`范围 ${fmtBytes(o.traffic_lower_bound_bytes)} ~ ${fmtBytes(o.traffic_upper_bound_bytes)}`}
        />
        <MetricCard
          label="活跃会话"
          value={String(o.sessions ?? 0)}
          sub={`${o.model_calls ?? 0} 次模型调用`}
          hint={`${o.nodes ?? 0} 节点 · ${o.collectors ?? 0} 采集器`}
        />
      </div>

      {/* 第二行：主趋势图 */}
      <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <div className="flex items-center justify-between mb-4 flex-wrap gap-3">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">使用趋势</h2>
          <div className="inline-flex rounded-lg bg-gray-100 dark:bg-gray-700/40 p-0.5">
            {TREND_TABS.map((t) => (
              <button
                key={t.key}
                type="button"
                onClick={() => setTrendTab(t.key)}
                className={`px-3 py-1.5 text-sm font-medium rounded-md ${trendTab === t.key ? 'bg-white dark:bg-gray-600 shadow-xs text-gray-800 dark:text-gray-100' : 'text-gray-500 dark:text-gray-400'}`}
              >
                {t.label}
              </button>
            ))}
          </div>
        </div>
        {trendData.labels.length === 0 ? <EmptyState title="当前范围无数据" /> : <TrendChart labels={trendData.labels} values={trendData.values} height={340} formatY={formatY} />}
      </div>

      {/* 第三行：双排行 */}
      <div className="mt-6 grid grid-cols-1 xl:grid-cols-2 gap-6">
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">模型调用排行</h2>
          <RankingList items={costItems} valueKey="value" labelKey="name" format={fmtTokensShort} limit={5} onItemClick={(m) => navigate(`/models/${encodeURIComponent(m.id)}`)} />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">Agent 使用分布</h2>
          <RankingList items={agentItems} valueKey="value" labelKey="name" format={fmtTokensShort} limit={5} onItemClick={(a) => navigate(`/agents/${encodeURIComponent(a.id)}`)} />
        </div>
      </div>

      {/* 第四行：关注事件 */}
      <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">需要关注</h2>
        </div>
        {alerts.length === 0 ? (
          <EmptyState title="暂无需要关注的事件" />
        ) : (
          <ul className="space-y-2">
            {alerts.map((a, i) => (
              <li key={i} className="flex items-center gap-3 px-3 py-2 rounded-lg bg-gray-50 dark:bg-gray-700/30 text-sm">
                <span className="w-2 h-2 rounded-full bg-amber-500 shrink-0" />
                <span className="text-gray-700 dark:text-gray-200">{a.text}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </>
  )
}
