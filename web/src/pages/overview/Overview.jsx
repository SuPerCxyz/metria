// 总览页：核心指标 + 主趋势图 + 模型/Agent 排行。

import React, { useCallback, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import MetricCard from '../../components/cards/MetricCard'
import TrendChart, { TOKEN_COLORS } from '../../components/charts/TrendChart'
import DailyUsageChart from '../../components/charts/DailyUsageChart'
import ActivityHeatmap from '../../components/charts/ActivityHeatmap'
import RankingList from '../../components/cards/RankingList'
import Segmented from '../../components/ui/Segmented'
import { ErrorState, LoadingSkeleton, EmptyState, ArchiveNotice } from '../../components/feedback/Feedback'
import { api, q, usageRangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useVisibleQuery } from '../../hooks/useVisibleQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { useNodeFilter } from '../../hooks/useNodeFilter'
import { currentWeekRange, previousTimeRange, withMinimumSpan } from '../../hooks/timeRangeState'
import { fmtTokensShort, fmtUsd, fmtTokens, fmtPct, fmtPct100, fmtDuration, fmtChange, changeTone, sumTokens, cacheHitRate, outputTokens, averageTokens } from '../../services/format'
import { isAvailable, performanceSourceLabel } from '../../services/dataAvailability'
import { ARCHIVED_UNAVAILABLE_LABEL, isActivityArchived } from '../../services/archiveRange'
import { formatTimeLabel } from '../../components/charts/trendChartLabels'

const TREND_TABS = [
  { key: 'tokens', label: 'Token' },
  { key: 'cost', label: '费用' },
  { key: 'requests', label: '请求数' },
]

const DIMS = [
  { key: 'all', label: '汇总' },
  { key: 'model', label: '按模型' },
  { key: 'client', label: '按 Agent' },
]

const EMPTY_HIDDEN = []

// 归档区间的活动指标提示（activity_archived_before 非 null 时展示）
const ARCHIVE_NOTICE = '所选时间早于归档水位，部分活动指标不可计算'

// 日趋势最小窗口 7 天：全局范围不足时补到最近 7 天，更长范围按所选显示。
const DAILY_MIN_SPAN_MS = 7 * 24 * 60 * 60 * 1000

export default function Overview() {
  const { range, setRange } = useTimeRange()
  const navigate = useNavigate()
  const { nodeId, clientId, model } = useNodeFilter()
  const params = usageRangeParams(range, { nodeId, clientId, model })
  const [trendTab, setTrendTab] = useState('tokens')
  const [dim, setDim] = useState('all')
  const [dailyMetric, setDailyMetric] = useState('tokens')
  const [heatmapMetric, setHeatmapMetric] = useState('tokens')
  const [selectedHeatmapCell, setSelectedHeatmapCell] = useState(null)
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
  const previousParams = usageRangeParams(previousRange, { nodeId, clientId, model })

  const overview = useQuery(`overview${q(overviewParams)}`, () => api(`/overview${q(overviewParams)}`))
  // 性能查询成本高：所在区域（消息与数据状态）可见后才发起，顶栏手动刷新仍会强制发起
  const performance = useVisibleQuery(`overview-performance${q(overviewParams)}`, () => api(`/usage/performance${q(overviewParams)}`))
  const previousOverview = useQuery(
    `overview-previous${q({ ...previousParams, ...excludedParams })}`,
    () => api(`/overview${q({ ...previousParams, ...excludedParams })}`),
    { enabled: Boolean(previousRange) }
  )
  const series = useQuery(`ts${q(seriesParams)}`, () => api(`/usage/timeseries${q(seriesParams)}`))
  const byDim = useQuery(`breakdown-cost${q({ ...params, dim: 'model' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'model' })}`))
  const byAgent = useQuery(`breakdown-client${q({ ...params, dim: 'client' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'client' })}`))
  const dailyRange = useMemo(() => withMinimumSpan(range, DAILY_MIN_SPAN_MS), [range])
  const dailyParams = usageRangeParams(dailyRange, { nodeId, clientId, model })
  const daily = useQuery(`daily${q(dailyParams)}`, () => api(`/usage/daily${q(dailyParams)}`))
  const heatmapRange = useMemo(() => currentWeekRange(range.timezone, new Date()), [range.timezone, range.to])
  const heatmapParams = usageRangeParams(heatmapRange, { nodeId, clientId, model })
  // 热力图查询成本高：所在卡片可见后才发起，顶栏手动刷新仍会强制发起
  const heatmap = useVisibleQuery(`heatmap${q(heatmapParams)}`, () => api(`/usage/heatmap${q(heatmapParams)}`))

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
            { label: '输入', color: TOKEN_COLORS.input, values: sorted.map((p) => p.input_tokens) },
            { label: '输出（含推理）', color: TOKEN_COLORS.output, values: sorted.map((p) => outputTokens(p)) },
            { label: '缓存读取', color: TOKEN_COLORS.cacheRead, values: sorted.map((p) => p.cache_read_tokens) },
            // 缓存写入为 0 时不占图层
            ...(sorted.some((p) => (p.cache_write_tokens ?? 0) > 0)
              ? [{ label: '缓存写入', color: TOKEN_COLORS.cacheWrite, values: sorted.map((p) => p.cache_write_tokens) }]
              : []),
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

  // 活动类字段的归档展示：水位非 null 且后端置 null → 诚实标注「已归档，不可计算」；
  // 水位为 null（旧后端）时下列判定恒为 false，渲染路径与改动前完全一致。
  const archivedMetric = (value) => isActivityArchived(o.activity_archived_before, value)
  const activityValue = (value, render) => (archivedMetric(value) ? ARCHIVED_UNAVAILABLE_LABEL : render())
  // 值不可计算时环比也失去意义：隐藏涨跌徽标，避免出现 -100% 之类的假涨跌
  const activityDelta = (value, previousValue) => (archivedMetric(value)
    ? { delta: null, deltaTone: 'neutral' }
    : compare(value, previousValue))
  const activityArchived = o.activity_archived_before != null

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
  const totalCost = (o.reported_cost_micro_usd ?? 0) + (o.calculated_cost_micro_usd ?? 0) + (o.estimated_cost_micro_usd ?? 0)
  const previousCost = previous
    ? (previous.reported_cost_micro_usd ?? 0) + (previous.calculated_cost_micro_usd ?? 0) + (previous.estimated_cost_micro_usd ?? 0)
    : null
  const pricingCoverage = o.pricing_coverage || {}
  const previousPricingCoverage = previous?.pricing_coverage || {}
  const activeSessions = o.active_sessions ?? null
  const previousActiveSessions = previous?.active_sessions ?? null
  const averageSessionCost = pricingCoverage.priced_calls > 0 && totalCost != null && activeSessions > 0
    ? totalCost / activeSessions
    : null
  const previousAverageSessionCost = previousPricingCoverage.priced_calls > 0 && previousCost != null && previousActiveSessions > 0
    ? previousCost / previousActiveSessions
    : null
  const performanceData = performance.data || {}
  const performanceCoverage = (key) => {
    const metric = performanceData[key]
    if (metric?.count == null) return '暂无观测数据'
    const base = `覆盖 ${fmtPct(metric.coverage)} · ${metric.count}/${performanceData.total_calls ?? 0} 次调用`
    const label = performanceSourceLabel(metric.sources)
    return label ? `${base} · ${label}` : base
  }
  const tokenTotal = sumTokens(o)
  const previousTokenTotal = previous ? sumTokens(previous) : null
  const averageTokenCount = o.token_calls ?? 0
  const averageTokensPerCall = averageTokens(tokenTotal, averageTokenCount)
  const previousAverageTokensPerCall = previous
    ? averageTokens(previousTokenTotal, previous.token_calls)
    : null
  const cacheRate = cacheHitRate(o)
  const hasCacheSavings = Number(o.cache_savings_micro_usd) > 0
  const performanceMetric = (key, valueKey = 'avg_ms') => {
    const metric = performanceData[key]
    return metric?.count > 0 && isAvailable(metric[valueKey])
  }
  const performanceCards = [
    <MetricCard key="ttft" span="xl:col-span-3" label="首个可观察输出" value={performanceMetric('ttft') ? fmtDuration(performanceData.ttft.avg_ms) : '—'} sub={performanceCoverage('ttft')} hint="调用开始到首个输出条目；日志推导为条目时间戳近似，缺失时保持不可用" />,
  ]

  return (
    <>
      <PageHeader title="总览" subtitle="AI 编程 Agent 用量、费用与性能概览" />

      <div className="mt-0">
        <h2 className="text-sm font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide mb-2">用量与成本</h2>
        <div className="grid grid-cols-12 gap-6">
          <MetricCard
            span="xl:col-span-4"
            label="Token 消耗"
            value={averageTokenCount > 0 ? fmtTokens(tokenTotal) : '—'}
            {...compare(averageTokenCount > 0 ? tokenTotal : null, previousAverageTokensPerCall != null ? previousTokenTotal : null)}
            sub={averageTokenCount > 0
              ? <span className="tabular-nums"><span className="text-gray-400 dark:text-gray-500">输入 {fmtTokensShort(o.input_tokens)} · 输出 {fmtTokensShort(outputTokens(o))}（其中推理 {fmtTokensShort(o.reasoning_tokens)}）· 缓存读取 {fmtTokensShort(o.cache_read_tokens)}{(o.cache_write_tokens ?? 0) > 0 ? ` · 缓存写入 ${fmtTokensShort(o.cache_write_tokens)}` : ''}</span></span>
              : '当前范围暂无 Token 数据'}
          />
          <MetricCard
            span="xl:col-span-4"
            label="缓存命中率"
            value={cacheRate != null ? fmtPct100(cacheRate) : '—'}
            {...compare(cacheRate, previous ? cacheHitRate(previous) : null)}
            sub={cacheRate != null ? '缓存读取 / (输入 + 缓存写入 + 缓存读取)' : '当前范围暂无缓存数据'}
            hint="缓存读取 Token 占请求上下文比例"
          />
          <MetricCard
            span="xl:col-span-4"
            label="平均每次调用 Token"
            value={averageTokensPerCall != null ? fmtTokensShort(averageTokensPerCall) : '—'}
            {...compare(averageTokensPerCall, previousAverageTokensPerCall)}
            sub={averageTokensPerCall != null ? `${averageTokenCount} 次有 Token 数据的调用` : '当前范围暂无 Token 数据'}
            hint="总 Token 除以有 Token 数据的调用数；缺失 Token 的调用不计入分母"
          />
          <MetricCard
            span="xl:col-span-4"
            label="总费用"
            value={pricingCoverage.priced_calls > 0 ? fmtUsd(totalCost) : '—'}
            {...compare(pricingCoverage.priced_calls > 0 ? totalCost : null, previousCost)}
            sub={pricingCoverage.priced_calls > 0 ? `${pricingCoverage.priced_calls} 次有费用口径调用` : '当前范围暂无费用口径'}
            hint="合计客户端上报、规则计算和估算三种费用口径；未定价调用不计入"
          />
          <MetricCard
            span="xl:col-span-4"
            label="缓存节省费用"
            value={hasCacheSavings ? fmtUsd(o.cache_savings_micro_usd) : '—'}
            {...compare(hasCacheSavings ? o.cache_savings_micro_usd : null, previous?.cache_savings_micro_usd)}
            sub={hasCacheSavings ? '按缓存读取单价估算' : '当前范围暂无缓存节省费用'}
            hint="缓存命中带来的成本节省"
          />
          <MetricCard
            span="xl:col-span-4"
            label="平均每会话费用"
            value={averageSessionCost != null
              ? fmtUsd(averageSessionCost)
              : archivedMetric(o.active_sessions)
                ? ARCHIVED_UNAVAILABLE_LABEL
                : '—'}
            {...(archivedMetric(o.active_sessions) ? { delta: null } : compare(averageSessionCost, previousAverageSessionCost))}
            sub={averageSessionCost != null
              ? `${activeSessions} 个活跃会话`
              : archivedMetric(o.active_sessions)
                ? '所选时间早于归档水位'
                : '当前范围暂无可计算费用'}
            hint="当前范围总费用除以范围内活跃或新建的会话数；无会话或未定价时保持不可用"
          />
        </div>
      </div>

      <div className="mt-3">
        <h2 className="text-sm font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide mb-2">活动与健康</h2>
        {activityArchived && (
          <div className="mb-3"><ArchiveNotice text={ARCHIVE_NOTICE} /></div>
        )}
        <div className="grid grid-cols-12 gap-6">
          <MetricCard span="xl:col-span-2" label="请求数" value={String(modelCalls)} {...compare(modelCalls, previousModelCalls)} sub="模型调用次数" hint={`涉及 ${o.models ?? 0} 个模型`} />
          <MetricCard span="xl:col-span-2" label="错误率" value={errorRate != null ? fmtPct100(errorRate) : '—'} {...compare(errorRate, previousErrorRate, true)} sub={errorRate != null ? `成功 ${fmtPct100(successRate)} · 失败 ${failed} 次` : '当前范围暂无请求数据'} hint="失败请求占全部请求比例；下降表示改善" />
          <MetricCard span="xl:col-span-2" label="活跃会话" value={activityValue(o.active_sessions, () => (isAvailable(o.active_sessions) ? String(o.active_sessions) : '—'))} {...activityDelta(o.active_sessions, previous?.active_sessions)} sub={archivedMetric(o.active_sessions) ? '所选时间早于归档水位' : (isAvailable(o.active_sessions) ? `${o.nodes ?? 0} 节点 · ${o.collectors ?? 0} 采集器` : '当前范围暂无会话数据')} hint="范围内新建或仍在活动的会话数" />
          <MetricCard span="xl:col-span-2" label="活跃时长" value={fmtDuration(o.active_duration_ms)} {...compare(o.active_duration_ms, previous?.active_duration_ms)} sub="模型调用耗时合计" hint="仅统计有明确 duration_ms 的调用，调用之间可能重叠" />
          <MetricCard span="xl:col-span-2" label="会话总时长" value={activityValue(o.session_duration_ms, () => fmtDuration(o.session_duration_ms))} {...activityDelta(o.session_duration_ms, previous?.session_duration_ms)} sub="范围重叠的会话跨度合计" hint="与所选范围重叠的会话跨度（裁剪到范围边界）；不去重重叠会话" />
          <MetricCard span="xl:col-span-2" label="节点在线" value={isAvailable(o.collectors) ? `${o.collectors_online ?? 0} / ${o.collectors}` : '—'} sub={isAvailable(o.collectors) ? `${o.nodes ?? 0} 节点 · ${o.projects ?? 0} 项目` : '当前范围暂无采集器数据'} hint="在线采集器 / 总数" />
        </div>
      </div>

      <div className="mt-3" ref={performance.ref}>
        <h2 className="text-sm font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide mb-2">消息与数据状态</h2>
        {activityArchived && (
          <div className="mb-3"><ArchiveNotice text={ARCHIVE_NOTICE} /></div>
        )}
        <div className="grid grid-cols-12 gap-6">
          <MetricCard span="xl:col-span-3" label="总消息数" value={activityValue(o.message_count, () => fmtTokensShort(o.message_count))} {...activityDelta(o.message_count, previous?.message_count)} sub="按消息时间统计（需内容采集）" />
          <MetricCard span="xl:col-span-3" label="用户消息数" value={activityValue(o.user_message_count, () => fmtTokensShort(o.user_message_count))} {...activityDelta(o.user_message_count, previous?.user_message_count)} sub="可识别 role=user" />
          <MetricCard span="xl:col-span-3" label="工具调用消息" value={activityValue(o.tool_call_count, () => fmtTokensShort(o.tool_call_count))} {...activityDelta(o.tool_call_count, previous?.tool_call_count)} sub="按工具事件时间统计" />
          {performanceCards}
        </div>
        {performance.loading && <p className="mt-2 text-xs text-gray-400 dark:text-gray-500">性能数据加载中…</p>}
        {performance.error && <p className="mt-2 text-xs text-amber-600 dark:text-amber-400">性能数据加载失败：{performance.error.message}</p>}
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
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5" ref={heatmap.ref}>
          <ActivityHeatmap
            cells={heatmap.data?.cells}
            metric={heatmapMetric}
            onMetricChange={setHeatmapMetric}
            loading={heatmap.loading}
            error={heatmap.error}
            selectedCell={selectedHeatmapCell}
            onCellClick={(cell) => {
              if (!cell.latest_from || !cell.latest_to) return
              setSelectedHeatmapCell(cell)
            }}
          />
          {selectedHeatmapCell?.latest_from && selectedHeatmapCell?.latest_to && (
            <div className="mt-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-indigo-100 bg-indigo-50/60 px-3 py-2 text-sm dark:border-indigo-900/60 dark:bg-indigo-500/10">
              <div className="text-gray-700 dark:text-gray-200">
                <span className="font-semibold">{formatHeatmapHour(selectedHeatmapCell)}</span>
                <span className="ml-3 text-gray-500 dark:text-gray-400">Token {fmtTokensShort(selectedHeatmapCell.tokens)} · 请求 {Number(selectedHeatmapCell.model_calls ?? 0).toLocaleString()}</span>
              </div>
              <button
                type="button"
                className="btn-xs btn-secondary"
                onClick={() => {
                  setRange({ from: selectedHeatmapCell.latest_from, to: selectedHeatmapCell.latest_to, timezone: range.timezone })
                  navigate('/analytics')
                }}
              >
                进入分析
              </button>
            </div>
          )}
        </div>
      </div>

      {(modelTokenItems.length > 0 || costItems.length > 0) && <div className="mt-3 grid grid-cols-1 xl:grid-cols-2 gap-6">
        {modelTokenItems.length > 0 && <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-3">模型 Token 排行</h2>
          <RankingList items={modelTokenItems} valueKey="value" labelKey="name" format={fmtTokensShort} secondaryKey="cost" secondaryFormat={fmtUsd} limit={5} onItemClick={(m) => navigate(`/models/${encodeURIComponent(m.id)}`)} />
        </div>}
        {costItems.length > 0 && <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-3">模型调用排行</h2>
          <RankingList items={costItems} valueKey="value" labelKey="name" format={(value) => value.toLocaleString()} secondaryKey="cost" secondaryFormat={fmtUsd} limit={5} onItemClick={(m) => navigate(`/models/${encodeURIComponent(m.id)}`)} />
        </div>}
      </div>}

      {(agentItems.length > 0 || agentTokenItems.length > 0) && <div className="mt-3 grid grid-cols-1 xl:grid-cols-2 gap-6">
        {agentItems.length > 0 && <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-3">Agent 使用分布</h2>
          <RankingList items={agentItems} valueKey="value" labelKey="name" format={(value) => value.toLocaleString()} secondaryKey="cost" secondaryFormat={fmtUsd} limit={5} onItemClick={(a) => navigate(`/agents/${encodeURIComponent(a.id)}`)} />
        </div>}
        {agentTokenItems.length > 0 && <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-3">Agent Token 排行</h2>
          <RankingList items={agentTokenItems} valueKey="value" labelKey="name" format={fmtTokensShort} secondaryKey="cost" secondaryFormat={fmtUsd} limit={5} onItemClick={(a) => navigate(`/agents/${encodeURIComponent(a.id)}`)} />
        </div>}
      </div>}

    </>
  )
}

function formatHeatmapHour(cell) {
  const weekday = ['周一', '周二', '周三', '周四', '周五', '周六', '周日'][cell.weekday] || '未知日期'
  return `${weekday} ${String(cell.hour).padStart(2, '0')}:00–${String(cell.hour).padStart(2, '0')}:59`
}
