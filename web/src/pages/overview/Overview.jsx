// 总览页：核心指标 + 主趋势图 + 模型/Agent 排行。

import React, { useCallback, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import MetricCard from '../../components/cards/MetricCard'
import TrendChart, { PALETTE as CHART_PALETTE } from '../../components/charts/TrendChart'
import RankingList from '../../components/cards/RankingList'
import Segmented from '../../components/ui/Segmented'
import { ErrorState, LoadingSkeleton, EmptyState } from '../../components/feedback/Feedback'
import { api, q, rangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { useNodeFilter } from '../../hooks/useNodeFilter'
import { fmtTokensShort, fmtUsd, fmtBytes, fmtTokens, fmtPct100, fmtDuration, sumTokensWithReasoning, cacheHitRate } from '../../services/format'

const TREND_TABS = [
  { key: 'tokens', label: 'Token' },
  { key: 'cost', label: '费用' },
  { key: 'traffic', label: '流量' },
  { key: 'requests', label: '请求数' },
]

const DIMS = [
  { key: 'all', label: '汇总' },
  { key: 'model', label: '按模型' },
  { key: 'client', label: '按 Agent' },
]

const EMPTY_HIDDEN = []

// x 轴显示时分（HH:mm）；tooltip 显示完整 MM/dd HH:mm
function fmtX(iso) {
  const d = new Date(iso)
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}
function fmtXFull(iso) {
  const d = new Date(iso)
  const p = (n) => String(n).padStart(2, '0')
  return `${p(d.getMonth() + 1)}/${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

export default function Overview() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const params = rangeParams(range)
  const { nodeId } = useNodeFilter()
  if (nodeId) params.node_id = nodeId
  const [trendTab, setTrendTab] = useState('tokens')
  const [dim, setDim] = useState('all')
  const [hiddenByDimension, setHiddenByDimension] = useState({ model: [], client: [] })
  const hiddenDimensions = hiddenByDimension[dim] || EMPTY_HIDDEN
  const excludedParams = dim === 'model'
    ? { exclude_models: hiddenDimensions.join(',') || undefined }
    : dim === 'client'
      ? { exclude_client_ids: hiddenDimensions.join(',') || undefined }
      : {}
  const overviewParams = { ...params, ...excludedParams }
  const seriesParams = { ...params, dim: dim === 'all' ? undefined : dim }

  const overview = useQuery(`overview${q(overviewParams)}`, () => api(`/overview${q(overviewParams)}`))
  const series = useQuery(`ts${q(seriesParams)}`, () => api(`/usage/timeseries${q(seriesParams)}`))
  const byDim = useQuery(`breakdown-cost${q({ ...params, dim: 'model' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'model' })}`))
  const byAgent = useQuery(`breakdown-client${q({ ...params, dim: 'client' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'client' })}`))

  const toggleHiddenDimension = useCallback((dimension) => {
    if (dim === 'all') return
    setHiddenByDimension((current) => {
      const hidden = current[dim] || []
      return {
        ...current,
        [dim]: hidden.includes(dimension)
          ? hidden.filter((item) => item !== dimension)
          : [...hidden, dimension],
      }
    })
  }, [dim])

  const metricOf = (p, tab) => {
    switch (tab) {
      case 'tokens': return (p.input_tokens || 0) + (p.output_tokens || 0) + (p.cache_read_tokens || 0)
      case 'cost': return p.cost_micro_usd
      case 'traffic': return p.estimated_traffic_bytes
      default: return p.model_calls
    }
  }

  const trendData = useMemo(() => {
    const pts = series.data?.series || []
    if (pts.length === 0) return { labels: [], datasets: [], tooltipLabels: [] }
    const sorted = pts.slice().sort((a, b) => (a.bucket < b.bucket ? -1 : 1))

    if (dim === 'all') {
      const labels = sorted.map((p) => fmtX(p.bucket))
      const tooltipLabels = sorted.map((p) => fmtXFull(p.bucket))
      if (trendTab === 'tokens') {
        return {
          labels,
          tooltipLabels,
          datasets: [
            { label: '输入', values: sorted.map((p) => p.input_tokens) },
            { label: '输出', values: sorted.map((p) => p.output_tokens) },
            { label: '缓存读取', values: sorted.map((p) => p.cache_read_tokens) },
          ],
        }
      }
      return { labels, tooltipLabels, datasets: [{ label: '', values: sorted.map((p) => metricOf(p, trendTab)) }] }
    }

    // 按维度拆分：每维度一条线（列出范围内全部有数据的维度，不再 Top5 截断）
    const byDimMap = {}
    for (const p of pts) {
      const k = p.dimension || '(未知)'
      ;(byDimMap[k] = byDimMap[k] || []).push(p)
    }
    const dims = Object.keys(byDimMap)
      .map((k) => ({ k, total: byDimMap[k].reduce((s, p) => s + metricOf(p, trendTab), 0) }))
      .filter((d) => d.total > 0)
      .sort((a, b) => b.total - a.total)
    const buckets = [...new Set(pts.map((p) => p.bucket))].sort()
    const labels = buckets.map(fmtX)
    const tooltipLabels = buckets.map(fmtXFull)
    const datasets = dims.map(({ k }) => {
      const map = {}
      for (const p of byDimMap[k]) map[p.bucket] = metricOf(p, trendTab)
      return { label: k, values: buckets.map((b) => map[b] || 0), hidden: hiddenDimensions.includes(k) }
    })
    return { labels, tooltipLabels, datasets }
  }, [series.data, trendTab, dim, hiddenDimensions])

  const formatY = (v) => {
    if (trendTab === 'tokens') return fmtTokensShort(v)
    if (trendTab === 'cost') return fmtUsd(v)
    if (trendTab === 'traffic') return fmtBytes(v)
    return v.toLocaleString()
  }

  if (overview.error) return <ErrorState error={overview.error} onRetry={overview.refresh} />
  if (overview.loading && !overview.data) return <LoadingSkeleton rows={6} />
  const o = overview.data || {}

  const costItems = (byDim.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '' && m.dimension !== '(unknown)')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: m.model_calls }))
    .filter((m) => m.value > 0)
  const modelTokenItems = (byDim.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '' && m.dimension !== '(unknown)')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: (m.input_tokens || 0) + (m.output_tokens || 0) + (m.cache_read_tokens || 0) }))
    .filter((m) => m.value > 0)
  const agentItems = (byAgent.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: m.model_calls }))
    .filter((m) => m.value > 0)
  const agentTokenItems = (byAgent.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: (m.input_tokens || 0) + (m.output_tokens || 0) + (m.cache_read_tokens || 0) }))
    .filter((m) => m.value > 0)

  const modelCalls = o.model_calls ?? 0
  const failed = o.failed_calls ?? 0
  const successRate = modelCalls > 0 ? ((modelCalls - failed) / modelCalls) * 100 : null

  return (
    <>
      <PageHeader title="总览" subtitle="AI 编程 Agent 用量、费用与流量概览" />

      {/* 第一行：用量 */}
      <div className="mt-0">
        <h2 className="text-sm font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide mb-2">用量</h2>
        <div className="grid grid-cols-12 gap-6">
          <MetricCard
            span="xl:col-span-4"
            label="总 Token"
            value={fmtTokens(sumTokensWithReasoning(o))}
            sub={
              <span className="tabular-nums">
                <span className="text-gray-400 dark:text-gray-500">输入 {fmtTokensShort(o.input_tokens)} · 输出 {fmtTokensShort(o.output_tokens)} · 缓存 {fmtTokensShort(o.cache_read_tokens)} · 推理 {fmtTokensShort(o.reasoning_tokens)}</span>
              </span>
            }
          />
          <MetricCard
            span="xl:col-span-4"
            label="缓存命中率"
            value={cacheHitRate(o) != null ? fmtPct100(cacheHitRate(o)) : '—'}
            sub="缓存读取 / (输入 + 缓存)"
            hint="缓存读取 Token 占请求上下文比例"
          />
          <MetricCard
            span="xl:col-span-4"
            label="调用时长"
            value={o.duration_p50_ms != null ? fmtDuration(o.duration_p50_ms) : '—'}
            sub={
              o.duration_p50_ms != null ? (
                <span className="tabular-nums">
                  P50 {fmtDuration(o.duration_p50_ms)} · P95 {fmtDuration(o.duration_p95_ms)} · P99 {fmtDuration(o.duration_p99_ms)}
                </span>
              ) : undefined
            }
            hint="整次调用时长分布（从发起到完成，非首 token 延迟）；客户端日志缺失时长则显示 —"
          />
        </div>
      </div>

      {/* 第二行：成本与流量 */}
      <div className="mt-3">
        <h2 className="text-sm font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide mb-2">成本与流量</h2>
        <div className="grid grid-cols-12 gap-6">
          <MetricCard
            span="xl:col-span-4"
            label="总费用"
            value={fmtUsd(o.calculated_cost_micro_usd ?? o.estimated_cost_micro_usd)}
            sub="估算费用"
            hint="按 Token 与价格目录估算"
          />
          <MetricCard
            span="xl:col-span-4"
            label="网络流量"
            value={fmtBytes(o.estimated_total_bytes)}
            sub="估算流量（含上下界）"
            hint={`范围 ${fmtBytes(o.traffic_lower_bound_bytes)} ~ ${fmtBytes(o.traffic_upper_bound_bytes)}`}
          />
          <MetricCard
            span="xl:col-span-4"
            label="缓存节省费用"
            value={o.cache_savings_micro_usd > 0 ? fmtUsd(o.cache_savings_micro_usd) : '—'}
            sub="按缓存读取单价估算"
            hint="缓存命中带来的成本节省"
          />
        </div>
      </div>

      {/* 第三行：活动与健康 */}
      <div className="mt-3">
        <h2 className="text-sm font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide mb-2">活动与健康</h2>
        <div className="grid grid-cols-12 gap-6">
          <MetricCard
            span="xl:col-span-3"
            label="请求数"
            value={String(modelCalls)}
            sub="模型调用次数"
            hint={`涉及 ${o.models ?? 0} 个模型`}
          />
          <MetricCard
            span="xl:col-span-3"
            label="成功率"
            value={successRate != null ? fmtPct100(successRate) : '—'}
            sub={`失败 ${failed} 次`}
            hint="成功请求占全部请求比例"
          />
          <MetricCard
            span="xl:col-span-3"
            label="新建会话"
            value={String(o.sessions ?? 0)}
            sub={`${o.nodes ?? 0} 节点 · ${o.collectors ?? 0} 采集器`}
            hint="当前 Agent 新建的会话数"
          />
          <MetricCard
            span="xl:col-span-3"
            label="节点在线"
            value={`${o.collectors_online ?? 0} / ${o.collectors ?? 0}`}
            sub={`${o.nodes ?? 0} 节点 · ${o.projects ?? 0} 项目`}
            hint="在线采集器 / 总数"
          />
        </div>
      </div>

      {/* 第二行：主趋势图 */}
      <div className="mt-3 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
        <div className="flex items-center justify-between mb-3 flex-wrap gap-3">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">使用趋势</h2>
          <div className="flex items-center gap-2 flex-wrap">
            <Segmented items={DIMS} value={dim} onChange={setDim} />
            <Segmented items={TREND_TABS} value={trendTab} onChange={setTrendTab} />
          </div>
        </div>
        {trendData.labels.length === 0 ? (
          <EmptyState title="当前范围无数据" />
        ) : (
          <>
            {dim !== 'all' && trendData.datasets.length > 0 && (
              <div className="mb-3">
                <div className="flex flex-wrap gap-1.5 max-h-28 overflow-y-auto items-start">
                  {trendData.datasets.map((d, i) => {
                    const hidden = hiddenDimensions.includes(d.label)
                    return (
                      <button
                        key={d.label}
                        type="button"
                        onClick={() => toggleHiddenDimension(d.label)}
                        className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs transition-colors ${
                          hidden
                            ? 'border-gray-200 dark:border-gray-700 text-gray-400 dark:text-gray-500 line-through opacity-60'
                            : 'border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-200 hover:bg-gray-50 dark:hover:bg-gray-700/40'
                        }`}
                        title={hidden ? '点击恢复该维度' : '点击隐藏该维度（汇总卡片将排除其数据）'}
                      >
                        <span className="w-2 h-2 rounded-full shrink-0" style={{ backgroundColor: CHART_PALETTE[i % CHART_PALETTE.length] }} />
                        <span className="max-w-48 truncate">{d.label}</span>
                      </button>
                    )
                  })}
                </div>
              </div>
            )}
            <TrendChart
              labels={trendData.labels}
              datasets={trendData.datasets}
              tooltipLabels={trendData.tooltipLabels}
              height={320}
              formatY={formatY}
              ariaLabel="使用趋势，点击图例可隐藏或恢复维度"
              onLegendClick={dim === 'all' ? undefined : toggleHiddenDimension}
              legendDisplay={dim === 'all'}
            />
          </>
        )}
      </div>

      {/* 第三行：模型排行（Token / 调用） */}
      <div className="mt-3 grid grid-cols-1 xl:grid-cols-2 gap-6">
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-3">模型 Token 排行</h2>
          <RankingList items={modelTokenItems} valueKey="value" labelKey="name" format={fmtTokensShort} limit={5} onItemClick={(m) => navigate(`/models/${encodeURIComponent(m.id)}`)} />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-3">模型调用排行</h2>
          <RankingList items={costItems} valueKey="value" labelKey="name" format={fmtTokensShort} limit={5} onItemClick={(m) => navigate(`/models/${encodeURIComponent(m.id)}`)} />
        </div>
      </div>

      {/* 第四行：Agent 排行（使用分布 / Token） */}
      <div className="mt-3 grid grid-cols-1 xl:grid-cols-2 gap-6">
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-3">Agent 使用分布</h2>
          <RankingList items={agentItems} valueKey="value" labelKey="name" format={fmtTokensShort} limit={5} onItemClick={(a) => navigate(`/agents/${encodeURIComponent(a.id)}`)} />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-3">Agent Token 排行</h2>
          <RankingList items={agentTokenItems} valueKey="value" labelKey="name" format={fmtTokensShort} limit={5} onItemClick={(a) => navigate(`/agents/${encodeURIComponent(a.id)}`)} />
        </div>
      </div>

    </>
  )
}
