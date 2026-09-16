// 总览页：核心指标 + 主趋势图 + 模型/Agent 排行。

import React, { useCallback, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import MetricCard from '../../components/cards/MetricCard'
import TrendChart from '../../components/charts/TrendChart'
import DailyUsageChart from '../../components/charts/DailyUsageChart'
import ActivityHeatmap from '../../components/charts/ActivityHeatmap'
import RankingList from '../../components/cards/RankingList'
import Segmented from '../../components/ui/Segmented'
import { ErrorState, LoadingSkeleton, EmptyState } from '../../components/feedback/Feedback'
import { api, q, usageRangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { useNodeFilter } from '../../hooks/useNodeFilter'
import { previousTimeRange, withMinimumSpan } from '../../hooks/timeRangeState'
import { fmtTokensShort, fmtUsd, fmtBytes, fmtTokens, fmtPct100, fmtDuration, fmtRelative, fmtChange, changeTone, sumTokens, cacheHitRate } from '../../services/format'
import { formatTimeLabel } from '../../components/charts/trendChartLabels'

const TREND_TABS = [
  { key: 'tokens', label: 'Token' },
  { key: 'cost', label: '费用' },
  { key: 'traffic', label: '估算流量' },
  { key: 'requests', label: '请求数' },
]

const DIMS = [
  { key: 'all', label: '汇总' },
  { key: 'model', label: '按模型' },
  { key: 'client', label: '按 Agent' },
]

const EMPTY_HIDDEN = []

// 日趋势最小窗口 7 天：全局范围不足时补到最近 7 天，更长范围按所选显示。
const DAILY_MIN_SPAN_MS = 7 * 24 * 60 * 60 * 1000

export default function Overview() {
  const { range, setRange } = useTimeRange()
  const navigate = useNavigate()
  const { nodeId, clientId, model, projectId } = useNodeFilter()
  const params = usageRangeParams(range, { nodeId, clientId, model, projectId })
  const [trendTab, setTrendTab] = useState('tokens')
  const [dim, setDim] = useState('all')
  const [dailyMetric, setDailyMetric] = useState('tokens')
  const [heatmapMetric, setHeatmapMetric] = useState('tokens')
  const [hiddenByDimension, setHiddenByDimension] = useState({ model: [], client: [] })
  const hiddenDimensions = hiddenByDimension[dim] || EMPTY_HIDDEN
  const excludedParams = dim === 'model'
    ? { exclude_models: hiddenDimensions.join(',') || undefined }
    : dim === 'client'
      ? { exclude_client_ids: hiddenDimensions.join(',') || undefined }
      : {}
  const overviewParams = { ...params, ...excludedParams }
  const seriesParams = { ...params, dim: dim === 'all' ? undefined : dim }
  const previousRange = useMemo(() => previousTimeRange(range), [range])
  const previousParams = usageRangeParams(previousRange, { nodeId, clientId, model, projectId })

  const overview = useQuery(`overview${q(overviewParams)}`, () => api(`/overview${q(overviewParams)}`))
  const previousOverview = useQuery(
    `overview-previous${q({ ...previousParams, ...excludedParams })}`,
    () => api(`/overview${q({ ...previousParams, ...excludedParams })}`),
    { enabled: Boolean(previousRange) }
  )
  const series = useQuery(`ts${q(seriesParams)}`, () => api(`/usage/timeseries${q(seriesParams)}`))
  const byDim = useQuery(`breakdown-cost${q({ ...params, dim: 'model' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'model' })}`))
  const byAgent = useQuery(`breakdown-client${q({ ...params, dim: 'client' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'client' })}`))
  const dailyRange = useMemo(() => withMinimumSpan(range, DAILY_MIN_SPAN_MS), [range])
  const dailyParams = usageRangeParams(dailyRange, { nodeId, clientId, model, projectId })
  const daily = useQuery(`daily${q(dailyParams)}`, () => api(`/usage/daily${q(dailyParams)}`))
  const heatmap = useQuery(`heatmap${q(params)}`, () => api(`/usage/heatmap${q(params)}`))

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
      case 'tokens': return sumTokens(p)
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
      const labels = sorted.map((p) => p.bucket)
      const tooltipLabels = sorted.map((p) => formatTimeLabel(p.bucket))
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
    const labels = buckets
    const tooltipLabels = buckets.map((bucket) => formatTimeLabel(bucket))
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
  const previous = previousOverview.data || null
  const compare = (current, previousValue, inverse = false) => ({
    delta: previous ? fmtChange(current, previousValue) : null,
    deltaTone: previous ? changeTone(current, previousValue, inverse) : 'neutral',
  })

  const costItems = (byDim.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '' && m.dimension !== '(unknown)')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: m.model_calls, cost: m.cost_micro_usd }))
    .filter((m) => m.value > 0)
  const modelTokenItems = (byDim.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '' && m.dimension !== '(unknown)')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: sumTokens(m), cost: m.cost_micro_usd }))
    .filter((m) => m.value > 0)
  const agentItems = (byAgent.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: m.model_calls, cost: m.cost_micro_usd }))
    .filter((m) => m.value > 0)
  const agentTokenItems = (byAgent.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: sumTokens(m), cost: m.cost_micro_usd }))
    .filter((m) => m.value > 0)

  const modelCalls = o.model_calls ?? 0
  const failed = o.failed_calls ?? 0
  const successRate = modelCalls > 0 ? ((modelCalls - failed) / modelCalls) * 100 : null
  const errorRate = modelCalls > 0 ? (failed / modelCalls) * 100 : null
  const previousModelCalls = previous?.model_calls ?? null
  const previousFailed = previous?.failed_calls ?? null
  const previousErrorRate = previousModelCalls > 0 ? (previousFailed / previousModelCalls) * 100 : null
  const totalCost = o.calculated_cost_micro_usd ?? o.estimated_cost_micro_usd
  const previousCost = previous?.calculated_cost_micro_usd ?? previous?.estimated_cost_micro_usd

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
            value={fmtTokens(sumTokens(o))}
            {...compare(sumTokens(o), previous ? sumTokens(previous) : null)}
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
            {...compare(cacheHitRate(o), previous ? cacheHitRate(previous) : null)}
            sub="缓存读取 / (输入 + 缓存写入 + 缓存读取)"
            hint="缓存读取 Token 占请求上下文比例"
          />
          <MetricCard
            span="xl:col-span-4"
            label="调用时长"
            value={o.duration_p50_ms != null ? fmtDuration(o.duration_p50_ms) : '—'}
            {...compare(o.duration_p50_ms, previous?.duration_p50_ms)}
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
            value={fmtUsd(totalCost)}
            {...compare(totalCost, previousCost)}
            sub="估算费用"
            hint="按 Token 与价格目录估算"
          />
          <MetricCard
            span="xl:col-span-4"
            label="估算流量"
            value={fmtBytes(o.estimated_total_bytes)}
            {...compare(o.estimated_total_bytes, previous?.estimated_total_bytes)}
            sub="估算流量（含上下界）"
            hint={`范围 ${fmtBytes(o.traffic_lower_bound_bytes)} ~ ${fmtBytes(o.traffic_upper_bound_bytes)}`}
          />
          <MetricCard
            span="xl:col-span-4"
            label="缓存节省费用"
            value={o.cache_savings_micro_usd > 0 ? fmtUsd(o.cache_savings_micro_usd) : '—'}
            {...compare(o.cache_savings_micro_usd, previous?.cache_savings_micro_usd)}
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
            span="xl:col-span-2"
            label="请求数"
            value={String(modelCalls)}
            {...compare(modelCalls, previousModelCalls)}
            sub="模型调用次数"
            hint={`涉及 ${o.models ?? 0} 个模型`}
          />
          <MetricCard
            span="xl:col-span-2"
            label="错误率"
            value={errorRate != null ? fmtPct100(errorRate) : '—'}
            {...compare(errorRate, previousErrorRate, true)}
            sub={`成功 ${successRate != null ? fmtPct100(successRate) : '—'} · 失败 ${failed} 次`}
            hint="失败请求占全部请求比例；下降表示改善"
          />
          <MetricCard
            span="xl:col-span-2"
            label="新建会话"
            value={String(o.sessions ?? 0)}
            {...compare(o.sessions, previous?.sessions)}
            sub={`${o.nodes ?? 0} 节点 · ${o.collectors ?? 0} 采集器`}
            hint="当前 Agent 新建的会话数"
          />
          <MetricCard
            span="xl:col-span-2"
            label="活跃时长"
            value={fmtDuration(o.active_duration_ms)}
            {...compare(o.active_duration_ms, previous?.active_duration_ms)}
            sub="模型调用耗时合计"
            hint="仅统计有明确 duration_ms 的调用，调用之间可能重叠"
          />
          <MetricCard
            span="xl:col-span-2"
            label="会话总时长"
            value={fmtDuration(o.session_duration_ms)}
            {...compare(o.session_duration_ms, previous?.session_duration_ms)}
            sub="会话持续跨度合计"
            hint="从会话开始到最后活动/结束；不去重重叠会话"
          />
          <MetricCard
            span="xl:col-span-2"
            label="节点在线"
            value={`${o.collectors_online ?? 0} / ${o.collectors ?? 0}`}
            sub={`${o.nodes ?? 0} 节点 · ${o.projects ?? 0} 项目`}
            hint="在线采集器 / 总数"
          />
        </div>
      </div>

      {/* 消息统计与数据新鲜度 */}
      <div className="mt-3">
        <h2 className="text-sm font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide mb-2">消息与数据状态</h2>
        <div className="grid grid-cols-12 gap-6">
          <MetricCard
            span="xl:col-span-3"
            label="总消息数"
            value={fmtTokensShort(o.message_count)}
            {...compare(o.message_count, previous?.message_count)}
            sub="用户、助手和系统消息"
          />
          <MetricCard
            span="xl:col-span-3"
            label="用户消息数"
            value={fmtTokensShort(o.user_message_count)}
            {...compare(o.user_message_count, previous?.user_message_count)}
            sub="可识别 role=user"
          />
          <MetricCard
            span="xl:col-span-3"
            label="工具调用消息"
            value={fmtTokensShort(o.tool_call_count)}
            {...compare(o.tool_call_count, previous?.tool_call_count)}
            sub="会话工具调用计数"
          />
          <FreshnessCard freshness={o.freshness} />
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
            <TrendChart
              labels={trendData.labels}
              datasets={trendData.datasets}
              tooltipLabels={trendData.tooltipLabels}
              range={range}
              height={320}
              formatY={formatY}
              ariaLabel="使用趋势，点击图例可隐藏或恢复维度"
              onLegendClick={dim === 'all' ? undefined : toggleHiddenDimension}
              legendDisplay={trendData.datasets.length > 0}
            />
          </>
        )}
      </div>

      <div className="mt-3 grid grid-cols-1 xl:grid-cols-2 gap-6">
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
          <DailyUsageChart
            series={daily.data?.series}
            range={dailyRange}
            metric={dailyMetric}
            onMetricChange={setDailyMetric}
            loading={daily.loading}
            error={daily.error}
          />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
          <ActivityHeatmap
            cells={heatmap.data?.cells}
            metric={heatmapMetric}
            onMetricChange={setHeatmapMetric}
            loading={heatmap.loading}
            error={heatmap.error}
            onCellClick={(cell) => {
              if (!cell.latest_from || !cell.latest_to) return
              setRange({ from: cell.latest_from, to: cell.latest_to, timezone: range.timezone })
              navigate('/analytics')
            }}
          />
        </div>
      </div>

      {/* 第三行：模型排行（Token / 调用） */}
      <div className="mt-3 grid grid-cols-1 xl:grid-cols-2 gap-6">
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-3">模型 Token 排行</h2>
          <RankingList items={modelTokenItems} valueKey="value" labelKey="name" format={fmtTokensShort} secondaryKey="cost" secondaryFormat={fmtUsd} limit={5} onItemClick={(m) => navigate(`/models/${encodeURIComponent(m.id)}`)} />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-3">模型调用排行</h2>
          <RankingList items={costItems} valueKey="value" labelKey="name" format={fmtTokensShort} secondaryKey="cost" secondaryFormat={fmtUsd} limit={5} onItemClick={(m) => navigate(`/models/${encodeURIComponent(m.id)}`)} />
        </div>
      </div>

      {/* 第四行：Agent 排行（使用分布 / Token） */}
      <div className="mt-3 grid grid-cols-1 xl:grid-cols-2 gap-6">
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-3">Agent 使用分布</h2>
          <RankingList items={agentItems} valueKey="value" labelKey="name" format={fmtTokensShort} secondaryKey="cost" secondaryFormat={fmtUsd} limit={5} onItemClick={(a) => navigate(`/agents/${encodeURIComponent(a.id)}`)} />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-3">Agent Token 排行</h2>
          <RankingList items={agentTokenItems} valueKey="value" labelKey="name" format={fmtTokensShort} secondaryKey="cost" secondaryFormat={fmtUsd} limit={5} onItemClick={(a) => navigate(`/agents/${encodeURIComponent(a.id)}`)} />
        </div>
      </div>

    </>
  )
}

function FreshnessCard({ freshness }) {
  const status = freshness?.status || 'unavailable'
  const statusText = { fresh: '正常', delayed: '有延迟', error: '有错误', unavailable: '不可用' }[status] || '不可用'
  const statusClass = status === 'fresh'
    ? 'text-emerald-600 bg-emerald-100 dark:text-emerald-400 dark:bg-emerald-400/10'
    : status === 'unavailable'
      ? 'text-gray-500 bg-gray-100 dark:text-gray-400 dark:bg-gray-700/40'
      : 'text-amber-600 bg-amber-100 dark:text-amber-400 dark:bg-amber-400/10'
  const latest = freshness?.last_event_at || freshness?.last_scan_at || freshness?.last_upload_at
  const coverage = freshness?.coverage == null ? '—' : `${(Number(freshness.coverage) * 100).toFixed(1)}%`
  return (
    <div className="flex flex-col col-span-full sm:col-span-6 xl:col-span-3 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
      <div className="flex items-center justify-between mb-2">
        <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400">数据新鲜度</h3>
        <span className={`rounded-full px-2 py-0.5 text-xs font-medium ${statusClass}`}>{statusText}</span>
      </div>
      <div className="text-2xl font-bold text-gray-800 dark:text-gray-100 tabular-nums tracking-tight">{fmtRelative(latest)}</div>
      <div className="mt-1 text-xs text-gray-400 dark:text-gray-500 tabular-nums">
        在线采集器 {freshness?.collectors_online ?? 0} / {freshness?.collectors_total ?? 0} · 来源覆盖率 {coverage}
      </div>
      <div className="mt-1 truncate text-xs text-gray-300 dark:text-gray-600" title={latest || undefined}>
        {freshness?.source_stale ? `${freshness.source_stale} 个来源延迟` : '最近扫描已完成'}
        {freshness?.source_errors ? ` · ${freshness.source_errors} 个来源错误` : ''}
      </div>
    </div>
  )
}
