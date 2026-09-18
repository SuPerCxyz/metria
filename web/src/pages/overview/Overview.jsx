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
import { ErrorState, LoadingSkeleton, EmptyState } from '../../components/feedback/Feedback'
import { api, q, usageRangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { useNodeFilter } from '../../hooks/useNodeFilter'
import { previousTimeRange, withMinimumSpan } from '../../hooks/timeRangeState'
import { fmtTokensShort, fmtUsd, fmtBytes, fmtTokens, fmtPct, fmtPct100, fmtDuration, fmtRelative, fmtChange, changeTone, sumTokens, cacheHitRate, outputTokens, averageTokens } from '../../services/format'
import { isAvailable } from '../../services/dataAvailability'
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
  const performance = useQuery(`overview-performance${q(overviewParams)}`, () => api(`/usage/performance${q(overviewParams)}`))
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
  const averageCallCost = pricingCoverage.priced_calls > 0 && totalCost != null
    ? totalCost / pricingCoverage.priced_calls
    : null
  const previousAverageCallCost = previousPricingCoverage.priced_calls > 0 && previousCost != null
    ? previousCost / previousPricingCoverage.priced_calls
    : null
  const performanceData = performance.data || {}
  const performanceCoverage = (key) => {
    const metric = performanceData[key]
    return metric?.count != null
      ? `覆盖 ${fmtPct(metric.coverage)} · ${metric.count}/${performanceData.total_calls ?? 0} 次调用`
      : '暂无实时观测数据'
  }
  const observedBytesCoverage = (key) => {
    const metric = performanceData.observed_bytes?.[key]
    return metric?.count > 0
      ? `${metric.count}/${performanceData.total_calls ?? 0} 次调用有观测`
      : '暂无实时观测数据'
  }
  const performanceSources = (performanceData.sources || [])
    .map((source) => `${source.source}（${source.quality}，${source.count}）`)
    .join('、') || '暂无实时观测样本'
  const tokenTotal = sumTokens(o)
  const previousTokenTotal = previous ? sumTokens(previous) : null
  const averageTokenCount = o.token_calls ?? 0
  const averageTokensPerCall = averageTokens(tokenTotal, averageTokenCount)
  const previousAverageTokensPerCall = previous
    ? averageTokens(previousTokenTotal, previous.token_calls)
    : null
  const cacheRate = cacheHitRate(o)
  const hasTokenData = averageTokenCount > 0
  const hasCacheSavings = Number(o.cache_savings_micro_usd) > 0
  const hasCostData = pricingCoverage.priced_calls > 0
  const hasActivityData = modelCalls > 0 || Number(o.sessions) > 0 || Number(o.collectors) > 0 || isAvailable(o.active_duration_ms) || isAvailable(o.session_duration_ms)
  const hasMessageData = Number(o.message_count) > 0 || Number(o.user_message_count) > 0 || Number(o.tool_call_count) > 0
  const performanceMetric = (key, valueKey = 'avg_ms') => {
    const metric = performanceData[key]
    return metric?.count > 0 && isAvailable(metric[valueKey])
  }
  const performanceCards = [
    performanceMetric('ttft') && <MetricCard key="ttft" span="xl:col-span-2" label="首个可观察输出" value={fmtDuration(performanceData.ttft.avg_ms)} sub={performanceCoverage('ttft')} hint="首 Token 延迟；普通 metria agent 未通过 observe 时不产生该样本" />,
    performanceMetric('first_byte') && <MetricCard key="first-byte" span="xl:col-span-2" label="首字节延迟" value={fmtDuration(performanceData.first_byte.avg_ms)} sub={performanceCoverage('first_byte')} hint="请求开始到首个响应字节的观测时长" />,
    performanceMetric('generation') && <MetricCard key="generation" span="xl:col-span-2" label="生成耗时" value={fmtDuration(performanceData.generation.avg_ms)} sub={performanceCoverage('generation')} hint="首 Token 到最后输出事件的观测时长" />,
    performanceMetric('output_speed', 'avg_tokens_per_second') && <MetricCard key="speed" span="xl:col-span-2" label="输出速度" value={`${performanceData.output_speed.avg_tokens_per_second.toFixed(1)} Token/s`} sub={performanceCoverage('output_speed')} hint="仅使用首 Token 后的生成区间计算" />,
    performanceMetric('inter_token_latency') && <MetricCard key="itl" span="xl:col-span-2" label="Token 间延迟" value={fmtDuration(performanceData.inter_token_latency.avg_ms)} sub={performanceCoverage('inter_token_latency')} hint="相邻输出 Token 之间的观测间隔" />,
    performanceData.stalls?.count > 0 && isAvailable(performanceData.stalls.avg_count) && <MetricCard key="stalls" span="xl:col-span-2" label="平均停顿次数" value={Number(performanceData.stalls.avg_count).toFixed(1)} sub={`${performanceData.stalls.count} 次调用有停顿统计`} hint="观测到的生成停顿次数，不包含未观测调用" />,
    ...[
      ['request_payload', '观测请求字节'],
      ['response_payload', '观测响应字节'],
      ['request_wire', '观测请求 Wire 字节'],
      ['response_wire', '观测响应 Wire 字节'],
    ].map(([key, label]) => {
      const metric = performanceData.observed_bytes?.[key]
      return metric?.count > 0 && isAvailable(metric.bytes)
        ? <MetricCard key={key} span="xl:col-span-2" label={label} value={fmtBytes(metric.bytes)} sub={observedBytesCoverage(key)} hint="仅表示运行时观测链路看到的字节，不是估算流量" />
        : null
    }),
  ].filter(Boolean)

  return (
    <>
      <PageHeader title="总览" subtitle="AI 编程 Agent 用量、费用与性能概览" />

      {hasTokenData && (
        <div className="mt-0">
          <h2 className="text-sm font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide mb-2">用量</h2>
          <div className="grid grid-cols-12 gap-6">
            <MetricCard
              span="xl:col-span-4"
              label="Token 消耗"
              value={fmtTokens(tokenTotal)}
              {...compare(tokenTotal, previousTokenTotal)}
              sub={
                <span className="tabular-nums">
                  <span className="text-gray-400 dark:text-gray-500">输入 {fmtTokensShort(o.input_tokens)} · 输出 {fmtTokensShort(outputTokens(o))}（其中推理 {fmtTokensShort(o.reasoning_tokens)}）· 缓存读取 {fmtTokensShort(o.cache_read_tokens)}{(o.cache_write_tokens ?? 0) > 0 ? ` · 缓存写入 ${fmtTokensShort(o.cache_write_tokens)}` : ''}</span>
                </span>
              }
            />
            {cacheRate != null && <MetricCard
              span="xl:col-span-4"
              label="缓存命中率"
              value={fmtPct100(cacheRate)}
              {...compare(cacheRate, previous ? cacheHitRate(previous) : null)}
              sub="缓存读取 / (输入 + 缓存写入 + 缓存读取)"
              hint="缓存读取 Token 占请求上下文比例"
            />}
            {averageTokensPerCall != null && <MetricCard
              span="xl:col-span-4"
              label="平均每次调用 Token"
              value={fmtTokensShort(averageTokensPerCall)}
              {...compare(averageTokensPerCall, previousAverageTokensPerCall)}
              sub={`${averageTokenCount} 次有 Token 数据的调用`}
              hint="总 Token 除以有 Token 数据的调用数；缺失 Token 的调用不计入分母"
            />}
          </div>
        </div>
      )}

      {hasCostData && (
        <div className="mt-3">
          <h2 className="text-sm font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide mb-2">成本</h2>
          <div className="grid grid-cols-12 gap-6">
            <MetricCard
              span="xl:col-span-4"
              label="总费用"
              value={fmtUsd(totalCost)}
              {...compare(totalCost, previousCost)}
              sub={`${pricingCoverage.priced_calls} 次有费用口径调用`}
              hint="合计客户端上报、规则计算和估算三种费用口径；未定价调用不计入"
            />
            {hasCacheSavings && <MetricCard
              span="xl:col-span-4"
              label="缓存节省费用"
              value={fmtUsd(o.cache_savings_micro_usd)}
              {...compare(o.cache_savings_micro_usd, previous?.cache_savings_micro_usd)}
              sub="按缓存读取单价估算"
              hint="缓存命中带来的成本节省"
            />}
            {averageCallCost != null && <MetricCard
              span="xl:col-span-4"
              label="平均单次调用费用"
              value={fmtUsd(averageCallCost)}
              {...compare(averageCallCost, previousAverageCallCost)}
              sub={`${pricingCoverage.priced_calls} 次有费用口径调用`}
              hint="当前范围总费用除以有费用口径的调用数；未定价调用不按 0 计入"
            />}
          </div>
        </div>
      )}

      {performance.loading ? <div className="mt-3"><LoadingSkeleton rows={2} /></div> : performance.error ? <div className="mt-3"><ErrorState error={performance.error} onRetry={performance.refresh} /></div> : performanceCards.length > 0 && (
        <div className="mt-3">
          <h2 className="text-sm font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide mb-2">实时性能观测</h2>
          <div className="grid grid-cols-12 gap-6">{performanceCards}</div>
          <p className="mt-2 text-xs text-gray-400 dark:text-gray-500">
            观测来源：{performanceSources}；普通日志调用缺失这些字段时保持不可用，不使用推算值补齐。
          </p>
        </div>
      )}

      {hasActivityData && (
        <div className="mt-3">
          <h2 className="text-sm font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide mb-2">活动与健康</h2>
          <div className="grid grid-cols-12 gap-6">
            {modelCalls > 0 && <MetricCard
              span="xl:col-span-2"
              label="请求数"
              value={String(modelCalls)}
              {...compare(modelCalls, previousModelCalls)}
              sub="模型调用次数"
              hint={`涉及 ${o.models ?? 0} 个模型`}
            />}
            {errorRate != null && <MetricCard
              span="xl:col-span-2"
              label="错误率"
              value={fmtPct100(errorRate)}
              {...compare(errorRate, previousErrorRate, true)}
              sub={`成功 ${fmtPct100(successRate)} · 失败 ${failed} 次`}
              hint="失败请求占全部请求比例；下降表示改善"
            />}
            {Number(o.sessions) > 0 && <MetricCard
              span="xl:col-span-2"
              label="新建会话"
              value={String(o.sessions)}
              {...compare(o.sessions, previous?.sessions)}
              sub={`${o.nodes ?? 0} 节点 · ${o.collectors ?? 0} 采集器`}
              hint="当前 Agent 新建的会话数"
            />}
            {isAvailable(o.active_duration_ms) && <MetricCard
              span="xl:col-span-2"
              label="活跃时长"
              value={fmtDuration(o.active_duration_ms)}
              {...compare(o.active_duration_ms, previous?.active_duration_ms)}
              sub="模型调用耗时合计"
              hint="仅统计有明确 duration_ms 的调用，调用之间可能重叠"
            />}
            {isAvailable(o.session_duration_ms) && <MetricCard
              span="xl:col-span-2"
              label="会话总时长"
              value={fmtDuration(o.session_duration_ms)}
              {...compare(o.session_duration_ms, previous?.session_duration_ms)}
              sub="会话持续跨度合计"
              hint="从会话开始到最后活动/结束；不去重重叠会话"
            />}
            {Number(o.collectors) > 0 && <MetricCard
              span="xl:col-span-2"
              label="节点在线"
              value={`${o.collectors_online ?? 0} / ${o.collectors}`}
              sub={`${o.nodes ?? 0} 节点 · ${o.projects ?? 0} 项目`}
              hint="在线采集器 / 总数"
            />}
          </div>
        </div>
      )}

      {(hasMessageData || o.freshness) && (
        <div className="mt-3">
          <h2 className="text-sm font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide mb-2">消息与数据状态</h2>
          <div className="grid grid-cols-12 gap-6">
            {hasMessageData && <>
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
            </>}
            <FreshnessCard freshness={o.freshness} />
          </div>
        </div>
      )}

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
