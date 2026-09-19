// 使用分析：Token / 请求 / 缓存 / 延迟 标签切换。

import React, { useCallback, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import MetricCard from '../../components/cards/MetricCard'
import TrendChart, { COLORS, PALETTE } from '../../components/charts/TrendChart'
import RankingList from '../../components/cards/RankingList'
import Segmented from '../../components/ui/Segmented'
import { ErrorState, LoadingSkeleton, EmptyState } from '../../components/feedback/Feedback'
import { api, q, usageRangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { useNodeFilter } from '../../hooks/useNodeFilter'
import { useNodeNames } from '../../hooks/useNodeNames'
import { previousTimeRange } from '../../hooks/timeRangeState'
import { fmtTokensShort, fmtUsd, fmtBytes, fmtPct, fmtPct100, fmtDuration, fmtChange, changeTone, sumTokens, cacheHitRate, outputTokens } from '../../services/format'
import { isAvailable, performanceSourceLabel } from '../../services/dataAvailability'

const TABS = [
  { key: 'tokens', label: 'Token' },
  { key: 'requests', label: '请求' },
  { key: 'cache', label: '缓存' },
  { key: 'latency', label: '延迟' },
  { key: 'agent-detail', label: '按 Agent' },
  { key: 'model-detail', label: '按模型' },
]

export default function Analytics() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const { nodeId, clientId, model, projectId } = useNodeFilter()
  const params = usageRangeParams(range, { nodeId, clientId, model, projectId })
  const [tab, setTab] = useState('tokens')
  const trendTab = 'tokens'
  const [hiddenByDimension, setHiddenByDimension] = useState({ client: [], model: [] })
  const [detailMetric, setDetailMetric] = useState('tokens')
  const detailMode = tab === 'agent-detail' || tab === 'model-detail'
  const detailDimension = tab === 'agent-detail' ? 'client' : 'model'
  const hiddenDimensions = hiddenByDimension[detailDimension]
  const excludedParams = detailMode
    ? detailDimension === 'client'
      ? { exclude_client_ids: hiddenDimensions.join(',') || undefined }
      : { exclude_models: hiddenDimensions.join(',') || undefined }
    : {}
  const scopedParams = detailMode
    ? { ...params, ...excludedParams }
    : params
  const detailSeriesParams = { ...params, dim: detailDimension }
  const previousRange = useMemo(() => previousTimeRange(range), [range])
  const previousParams = usageRangeParams(previousRange, { nodeId, clientId, model, projectId })
  const previousScopedParams = { ...previousParams, ...excludedParams }

  const overview = useQuery(`overview${q(scopedParams)}`, () => api(`/overview${q(scopedParams)}`))
  const previousOverview = useQuery(
    `analytics-previous${q(previousScopedParams)}`,
    () => api(`/overview${q(previousScopedParams)}`),
    { enabled: Boolean(previousRange) }
  )
  const series = useQuery(`ts${q(scopedParams)}`, () => api(`/usage/timeseries${q(scopedParams)}`))
  const byModel = useQuery(`breakdown${q({ ...params, dim: 'model' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'model' })}`))
  const byAgent = useQuery(`bclient${q({ ...params, dim: 'client' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'client' })}`))
  const byNode = useQuery(`bnode${q({ ...params, dim: 'node' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'node' })}`))
  const detailSeries = useQuery(
    `detail-ts${q(detailSeriesParams)}`,
    () => api(`/usage/timeseries${q(detailSeriesParams)}`),
    { enabled: detailMode }
  )
  const latency = useQuery(`latency${q(scopedParams)}`, () => api(`/usage/latency${q(scopedParams)}`))
  const latencySeries = useQuery(`latency-ts${q(scopedParams)}`, () => api(`/usage/latency/timeseries${q(scopedParams)}`))
  const performance = useQuery(`performance${q(scopedParams)}`, () => api(`/usage/performance${q(scopedParams)}`))
  const performanceSeries = useQuery(`performance-ts${q(scopedParams)}`, () => api(`/usage/performance/timeseries${q(scopedParams)}`))
  const nodeNames = useNodeNames()

  const latencyTrend = useMemo(() => {
    const pts = latencySeries.data?.series || []
    const labels = pts.map((p) => p.bucket)
    const pick = (key) => pts.map((p) => p[key] ?? null)
    return {
      labels,
      datasets: [
        { label: 'P50', values: pick('p50_ms'), color: COLORS.latency.p50 },
        { label: 'P95', values: pick('p95_ms'), color: COLORS.latency.p95 },
        { label: '平均', values: pick('avg_ms'), color: COLORS.latency.avg },
      ],
    }
  }, [latencySeries.data])

  const performanceTrend = useMemo(() => {
    const pts = performanceSeries.data?.series || []
    const pick = (key) => pts.map((p) => p[key] ?? null)
    return {
      labels: pts.map((p) => p.bucket),
      latency: [
        { label: 'TTFT', values: pick('ttft_avg_ms'), color: COLORS.latency.p50 },
        { label: '首字节', values: pick('first_byte_avg_ms'), color: COLORS.latency.p95 },
        { label: '生成耗时', values: pick('generation_avg_ms'), color: COLORS.latency.avg },
        { label: 'Token 间延迟', values: pick('inter_token_latency_avg_ms'), color: PALETTE[3] },
      ],
      speed: [{ label: '输出 Token/s', values: pts.map((p) => p.output_tokens_per_second_milli_avg == null ? null : p.output_tokens_per_second_milli_avg / 1000), color: COLORS.output }],
    }
  }, [performanceSeries.data])

  const trendData = useMemo(() => {
    const pts = series.data?.series || []
    switch (trendTab) {
      case 'tokens': return { labels: pts.map((p) => p.bucket), values: pts.map((p) => sumTokens(p)) }
      case 'cost': return { labels: pts.map((p) => p.bucket), values: pts.map((p) => p.cost_micro_usd) }
      default: return { labels: pts.map((p) => p.bucket), values: pts.map((p) => p.model_calls) }
    }
  }, [series.data, trendTab])

  const detailTrend = useMemo(() => {
    const points = detailSeries.data?.series || []
    const labels = [...new Set(points.map((p) => p.bucket))]
    const dimensions = [...new Set(points.map((p) => p.dimension).filter(Boolean))].slice(0, 8)
    const valueOf = (point) => {
      switch (detailMetric) {
        case 'requests': return point?.model_calls ?? 0
        case 'cost': return point?.cost_micro_usd ?? 0
        default: return sumTokens(point)
      }
    }
    const pointMap = new Map(points.map((point) => [String(point.dimension) + '|' + String(point.bucket), point]))
    return {
      labels,
      datasets: dimensions.map((dimension, index) => ({
        label: dimension,
        values: labels.map((label) => valueOf(pointMap.get(String(dimension) + '|' + String(label)))),
        color: DETAIL_COLORS[index % DETAIL_COLORS.length],
        hidden: hiddenDimensions.includes(dimension),
      })),
    }
  }, [detailMetric, detailSeries.data, hiddenDimensions])

  const toggleHiddenDimension = useCallback((dimension) => {
    setHiddenByDimension((current) => {
      const hidden = current[detailDimension] || []
      return {
        ...current,
        [detailDimension]: hidden.includes(dimension)
          ? hidden.filter((item) => item !== dimension)
          : [...hidden, dimension],
      }
    })
  }, [detailDimension])

  if (overview.error && !overview.data) return <ErrorState error={overview.error} onRetry={overview.refresh} />
  if (overview.loading && !overview.data) return <LoadingSkeleton rows={6} />
  const o = overview.data || {}
  const previous = previousOverview.data || null
  const compare = (current, previousValue, inverse = false) => ({
    delta: previous ? fmtChange(current, previousValue) : null,
    deltaTone: previous ? changeTone(current, previousValue, inverse) : 'neutral',
  })

  const cacheHit = cacheHitRate(o)
  const hasTokenData = (o.token_calls ?? 0) > 0
  const hasRequestData = (o.model_calls ?? 0) > 0
  const hasCacheData = cacheHit != null || (o.cache_read_tokens ?? 0) > 0 || (o.cache_write_tokens ?? 0) > 0
  const performanceMetric = (key, valueKey = 'avg_ms') => {
    const metric = performance.data?.[key]
    return metric?.count > 0 && isAvailable(metric[valueKey])
  }
  const performanceHasSamples = [
    performanceMetric('ttft'),
    performanceMetric('first_byte'),
    performanceMetric('generation'),
    performanceMetric('output_speed', 'avg_tokens_per_second'),
    performanceMetric('inter_token_latency'),
    performance.data?.stalls?.count > 0,
    ...['request_payload', 'response_payload', 'request_wire', 'response_wire'].map((key) => performance.data?.observed_bytes?.[key]?.count > 0),
  ].some(Boolean)

  const modelItems = (byModel.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '' && m.dimension !== '(unknown)')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: sumTokens(m), cost: m.cost_micro_usd }))
  const agentItems = (byAgent.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: sumTokens(m), cost: m.cost_micro_usd }))
  const nodeItems = (byNode.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '')
    .map((m) => ({ id: m.dimension, name: nodeNames[m.dimension] || m.dimension, value: sumTokens(m), cost: m.cost_micro_usd }))

  return (
    <>
      <PageHeader title="使用分析" subtitle="深入分析 Token、请求、缓存与延迟" />
      <div className="mb-3">
        <Segmented items={TABS} value={tab} onChange={setTab} />
      </div>

      {tab === 'tokens' && (
        hasTokenData ? <>
          <div className="grid grid-cols-12 gap-6">
            <MetricCard label="Token 消耗" value={fmtTokensShort(sumTokens(o))} {...compare(sumTokens(o), previous ? sumTokens(previous) : null)} sub={`输入 ${fmtTokensShort(o.input_tokens)} · 输出 ${fmtTokensShort(outputTokens(o))} · 缓存 ${fmtTokensShort((o.cache_read_tokens ?? 0) + (o.cache_write_tokens ?? 0))}`} />
            <MetricCard label="输入 Token" value={fmtTokensShort(o.input_tokens)} {...compare(o.input_tokens, previous?.input_tokens)} sub="请求上下文" />
            <MetricCard label="输出 Token" value={fmtTokensShort(outputTokens(o))} {...compare(outputTokens(o), previous ? outputTokens(previous) : null)} sub="模型生成（含推理）" />
            <MetricCard label="缓存 Token" value={fmtTokensShort(o.cache_read_tokens)} {...compare(o.cache_read_tokens, previous?.cache_read_tokens)} sub="缓存读取" />
          </div>
          <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
            <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">Token 趋势</h2>
            <TrendChart labels={trendData.labels} values={trendData.values} range={range} height={320} formatY={fmtTokensShort} />
          </div>
          <div className="mt-4 grid grid-cols-1 xl:grid-cols-3 gap-6">
            <RankingCard title="Agent Token 排行" items={agentItems} onClick={(i) => navigate(`/agents/${encodeURIComponent(i.id)}`)} />
            <RankingCard title="模型 Token 排行" items={modelItems} onClick={(i) => navigate(`/models/${encodeURIComponent(i.id)}`)} />
            <RankingCard title="节点 Token 排行" items={nodeItems} onClick={(i) => navigate(`/nodes/${encodeURIComponent(i.id)}`)} />
          </div>
        </> : <EmptyState title="当前范围暂无 Token 数据" desc="所选范围没有带有效 Token 的模型调用。" />
      )}

      {tab === 'requests' && (
        hasRequestData ? <>
          <div className="grid grid-cols-12 gap-6">
            <MetricCard label="请求总数" value={fmtTokensShort(o.model_calls)} {...compare(o.model_calls, previous?.model_calls)} />
            <MetricCard label="成功请求" value={fmtTokensShort((o.model_calls ?? 0) - (o.failed_calls ?? 0))} {...compare((o.model_calls ?? 0) - (o.failed_calls ?? 0), previous ? (previous.model_calls ?? 0) - (previous.failed_calls ?? 0) : null)} sub={`失败 ${o.failed_calls ?? 0} 次`} />
            <MetricCard label="失败请求" value={fmtTokensShort(o.failed_calls ?? 0)} {...compare(o.failed_calls, previous?.failed_calls)} sub="状态非成功" />
            <MetricCard label="平均请求频率" value={o.sessions > 0 ? ((o.model_calls ?? 0) / o.sessions).toFixed(1) : '—'} {...compare(o.sessions > 0 ? (o.model_calls ?? 0) / o.sessions : null, previous?.sessions > 0 ? (previous.model_calls ?? 0) / previous.sessions : null)} sub="每会话请求数" />
          </div>
          <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
            <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">请求趋势</h2>
            <TrendChart labels={trendData.labels} values={trendData.values} range={range} height={320} formatY={(v) => v.toLocaleString()} />
          </div>
        </> : <EmptyState title="当前范围暂无请求数据" desc="所选范围没有模型调用。" />
      )}

      {tab === 'cache' && (
        hasCacheData ? <>
          <div className="grid grid-cols-12 gap-6">
            <MetricCard label="缓存 Token" value={fmtTokensShort(o.cache_read_tokens)} {...compare(o.cache_read_tokens, previous?.cache_read_tokens)} sub="缓存读取" />
            <MetricCard label="缓存命中率" value={fmtPct100(cacheHit)} {...compare(cacheHit, cacheHitRate(previous))} />
            <MetricCard label="缓存节省费用" value={o.cache_savings_micro_usd > 0 ? fmtUsd(o.cache_savings_micro_usd) : '—'} {...compare(o.cache_savings_micro_usd, previous?.cache_savings_micro_usd)} sub={o.cache_savings_micro_usd > 0 ? '按缓存读取单价估算' : '无价格规则或缓存数据'} />
            {(o.cache_write_tokens ?? 0) > 0 && <MetricCard label="缓存写入" value={fmtTokensShort(o.cache_write_tokens)} {...compare(o.cache_write_tokens, previous?.cache_write_tokens)} />}
          </div>
          <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
            <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">缓存趋势</h2>
            <TrendChart labels={trendData.labels} values={trendData.values} range={range} height={320} formatY={fmtTokensShort} />
          </div>
        </> : <EmptyState title="当前范围暂无缓存数据" desc="所选范围没有可识别的缓存读取或缓存写入 Token。" />
      )}

      {detailMode && (
        <DetailAnalysis
          tab={tab}
          range={range}
          metric={detailMetric}
          onMetricChange={setDetailMetric}
          overview={o}
          previousOverview={previous}
          loading={detailSeries.loading}
          error={detailSeries.error}
          onRetry={detailSeries.refresh}
          trend={detailTrend}
          latency={latency.data}
          onToggleDimension={toggleHiddenDimension}
        />
      )}

      {tab === 'latency' && (
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">性能分析</h2>
          {performance.loading ? <LoadingSkeleton rows={2} /> : performance.error ? (
            <ErrorState error={performance.error} onRetry={performance.refresh} />
          ) : performanceHasSamples ? (
            <>
              <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
                {performanceMetric('ttft') && <PerformanceCard
                  label="TTFT"
                  value={fmtDuration(performance.data.ttft.avg_ms)}
                  count={performance.data?.ttft?.count}
                  total={performance.data?.total_calls}
                  coverage={performance.data?.ttft?.coverage}
                  sourceLabel={performanceSourceLabel(performance.data?.ttft?.sources)}
                  hint="优先使用原生实时首 Token；日志来源为条目时间戳近似。"
                />}
                {performanceMetric('output_speed', 'avg_tokens_per_second') && <PerformanceCard
                  label="输出速度"
                  value={`${performance.data.output_speed.avg_tokens_per_second.toFixed(1)} Token/s`}
                  count={performance.data?.output_speed?.count}
                  total={performance.data?.total_calls}
                  coverage={performance.data?.output_speed?.coverage}
                  sourceLabel={performanceSourceLabel(performance.data?.output_speed?.sources)}
                  hint="只计算首 Token 之后的生成区间，不把等待时间算入速度。"
                />}
                {performanceMetric('first_byte') && <PerformanceCard
                  label="首字节延迟"
                  value={fmtDuration(performance.data.first_byte.avg_ms)}
                  count={performance.data?.first_byte?.count}
                  total={performance.data?.total_calls}
                  coverage={performance.data?.first_byte?.coverage}
                  sourceLabel={performanceSourceLabel(performance.data?.first_byte?.sources)}
                  hint="请求开始到首个响应字节，仅原生 observe 可得。"
                />}
                {performanceMetric('generation') && <PerformanceCard
                  label="生成耗时"
                  value={fmtDuration(performance.data.generation.avg_ms)}
                  count={performance.data?.generation?.count}
                  total={performance.data?.total_calls}
                  coverage={performance.data?.generation?.coverage}
                  sourceLabel={performanceSourceLabel(performance.data?.generation?.sources)}
                  hint="首 Token 到最后输出事件。"
                />}
                {performanceMetric('inter_token_latency') && <PerformanceCard
                  label="Token 间延迟"
                  value={fmtDuration(performance.data.inter_token_latency.avg_ms)}
                  count={performance.data.inter_token_latency.count}
                  total={performance.data.total_calls}
                  coverage={performance.data.inter_token_latency.coverage}
                  sourceLabel={performanceSourceLabel(performance.data?.inter_token_latency?.sources)}
                  hint="相邻输出 Token 之间的观测间隔，仅原生 observe 可得。"
                />}
              </div>
              <div className="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
                {performance.data?.reliability?.success_rate != null && <SimpleMetric label="成功率" value={`${(performance.data.reliability.success_rate * 100).toFixed(1)}%`} />}
                {performance.data?.stalls?.count > 0 && isAvailable(performance.data.stalls.avg_count) && <SimpleMetric label="平均停顿次数" value={Number(performance.data.stalls.avg_count).toFixed(1)} />}
                {['request_payload', 'response_payload', 'request_wire', 'response_wire'].map((key) => {
                  const metric = performance.data?.observed_bytes?.[key]
                  if (metric?.count <= 0 || !isAvailable(metric.bytes)) return null
                  const labels = {
                    request_payload: '观测请求字节',
                    response_payload: '观测响应字节',
                    request_wire: '观测请求 Wire 字节',
                    response_wire: '观测响应 Wire 字节',
                  }
                  return <SimpleMetric key={key} label={labels[key]} value={fmtBytes(metric.bytes)} />
                })}
              </div>
              <p className="mt-3 text-xs text-gray-400 dark:text-gray-500">
                观测来源：{formatTimingSources(performance.data?.sources)}；不支持的调用保持不可用，不按 0 计入。
              </p>
              {performanceSeries.loading ? <LoadingSkeleton rows={3} /> : performanceSeries.error ? (
                <ErrorState error={performanceSeries.error} onRetry={performanceSeries.refresh} />
              ) : (
                <div className="mt-6 space-y-6">
                  {performanceTrend.latency.some((dataset) => dataset.values.some((value) => value != null)) && <div>
                    <h3 className="mb-3 text-sm font-semibold text-gray-500 dark:text-gray-400">性能趋势</h3>
                    <TrendChart labels={performanceTrend.labels} datasets={performanceTrend.latency} range={range} height={260} formatY={fmtDuration} ariaLabel="首字节、首 Token、生成耗时和 Token 间延迟趋势" />
                  </div>}
                  {performanceTrend.speed[0].values.some((value) => value != null) && <div>
                    <h3 className="mb-3 text-sm font-semibold text-gray-500 dark:text-gray-400">输出速度趋势</h3>
                    <TrendChart labels={performanceTrend.labels} datasets={performanceTrend.speed} range={range} height={220} formatY={(value) => `${Number(value).toFixed(1)} Token/s`} ariaLabel="输出 Token 每秒趋势" />
                  </div>}
                </div>
              )}
            </>
          ) : (
            <EmptyState title="暂无可用观测数据" desc="日志推导依赖客户端可证明事件；首字节、Token 间延迟、停顿与观测字节需原生 Agent 通过 observe 启动。" />
          )}

          {(latency.data?.count ?? 0) > 0 && <>
          <h3 className="mb-3 mt-6 text-sm font-semibold text-gray-500 dark:text-gray-400">日志端到端调用时长</h3>
          {latency.loading ? <LoadingSkeleton rows={3} /> : latency.error ? (
            <ErrorState error={latency.error} onRetry={latency.refresh} />
          ) : (
            <>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
                <LatencyCard label="P50" ms={latency.data.p50_ms} />
                <LatencyCard label="P95" ms={latency.data.p95_ms} />
                <LatencyCard label="P99" ms={latency.data.p99_ms} />
                <LatencyCard label="平均" ms={latency.data.avg_ms} />
              </div>
              <div className="mt-4">
                <h3 className="text-sm font-semibold text-gray-500 dark:text-gray-400 mb-3">延迟趋势</h3>
                {latencySeries.loading ? (
                  <LoadingSkeleton rows={3} />
                ) : latencySeries.error ? (
                  <ErrorState error={latencySeries.error} onRetry={latencySeries.refresh} />
                ) : (latencyTrend.datasets[0].values.some((v) => v != null)) ? (
                  <TrendChart
                    labels={latencyTrend.labels}
                    datasets={latencyTrend.datasets}
                    range={range}
                    height={300}
                    formatY={fmtDuration}
                    ariaLabel="延迟趋势，P50、P95 与平均时长随时间变化"
                  />
                ) : (
                  <EmptyState title="暂无延迟数据" desc="所选范围内客户端日志未记录调用时长（duration_ms 缺失）。" />
                )}
              </div>
            </>
          )}
          </>}
        </div>
      )}
    </>
  )
}

const DETAIL_COLORS = PALETTE

function DetailAnalysis({
  tab,
  range,
  metric,
  onMetricChange,
  overview,
  previousOverview,
  loading,
  error,
  onRetry,
  trend,
  latency,
  onToggleDimension,
}) {
  const metricItems = [
    { key: 'tokens', label: 'Token' },
    { key: 'requests', label: '请求' },
    { key: 'cost', label: '费用' },
  ]
  const formatY = metric === 'cost'
    ? fmtUsd
    : metric === 'requests'
        ? (value) => value.toLocaleString()
        : fmtTokensShort
  const title = tab === 'agent-detail' ? '按 Agent 详细分析' : '按模型详细分析'
  const chartTitle = tab === 'agent-detail' ? 'Agent 趋势对比' : '模型趋势对比'
  const detailDelta = (current, previousValue) => ({
    delta: previousOverview ? fmtChange(current, previousValue) : null,
    deltaTone: previousOverview ? changeTone(current, previousValue) : 'neutral',
  })

  return (
    <>
      <div className="grid grid-cols-12 gap-6">
        {sumTokens(overview) > 0 && <MetricCard label="范围 Token" value={fmtTokensShort(sumTokens(overview))} {...detailDelta(sumTokens(overview), previousOverview ? sumTokens(previousOverview) : null)} sub={`请求 ${fmtTokensShort(overview.model_calls)}`} />}
        {(overview.model_calls ?? 0) > 0 && <MetricCard label="请求数" value={fmtTokensShort(overview.model_calls)} {...detailDelta(overview.model_calls, previousOverview?.model_calls)} sub={`会话 ${fmtTokensShort(overview.sessions)}`} />}
        {isAvailable(overview.calculated_cost_micro_usd ?? overview.estimated_cost_micro_usd) && <MetricCard label="费用" value={fmtUsd(overview.calculated_cost_micro_usd ?? overview.estimated_cost_micro_usd)} {...detailDelta(overview.calculated_cost_micro_usd ?? overview.estimated_cost_micro_usd, previousOverview?.calculated_cost_micro_usd ?? previousOverview?.estimated_cost_micro_usd)} sub="当前范围" />}
        {latency?.avg_ms != null && <MetricCard label="平均日志时长" value={fmtDuration(latency.avg_ms)} sub="有记录的请求" />}
      </div>

      <div className="mt-4 rounded-2xl border border-gray-200 bg-white p-6 shadow-xs dark:border-gray-700/60 dark:bg-gray-800">
        <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">{title}</h2>
            <p className="mt-1 text-xs text-gray-400 dark:text-gray-500">{chartTitle} · 最多展示 8 个维度</p>
          </div>
          <Segmented items={metricItems} value={metric} onChange={onMetricChange} />
        </div>
        {loading ? <LoadingSkeleton rows={4} /> : error ? <ErrorState error={error} onRetry={onRetry} /> : trend.datasets.length > 0 ? (
          <TrendChart
            labels={trend.labels}
            datasets={trend.datasets}
            range={range}
            height={340}
            formatY={formatY}
            ariaLabel={`${chartTitle}，点击图例可隐藏或恢复维度`}
            onLegendClick={onToggleDimension}
          />
        ) : (
          <EmptyState title="暂无分析数据" desc="当前时间范围和筛选条件下没有可展示的数据。" />
        )}
      </div>
    </>
  )
}

function LatencyCard({ label, ms }) {
  return (
    <div className="bg-gray-50 dark:bg-gray-700/30 rounded-xl p-4">
      <div className="text-xs text-gray-400 dark:text-gray-500">{label}</div>
      <div className="mt-1 text-xl font-bold text-gray-800 dark:text-gray-100 tabular-nums">{ms != null ? fmtDuration(ms) : '—'}</div>
    </div>
  )
}

function SimpleMetric({ label, value }) {
  return (
    <div className="bg-gray-50 dark:bg-gray-700/30 rounded-xl p-4">
      <div className="text-xs text-gray-400 dark:text-gray-500">{label}</div>
      <div className="mt-1 text-xl font-bold text-gray-800 dark:text-gray-100 tabular-nums">{value}</div>
    </div>
  )
}

function PerformanceCard({ label, value, count = 0, total = 0, coverage, hint, sourceLabel }) {
  return (
    <div className="rounded-xl bg-gray-50 p-4 dark:bg-gray-700/30">
      <div className="text-xs text-gray-400 dark:text-gray-500">{label}</div>
      <div className="mt-1 text-xl font-bold tabular-nums text-gray-800 dark:text-gray-100">{value}</div>
      <div className="mt-1 text-xs text-gray-500 dark:text-gray-400">
        覆盖率 {fmtPct(coverage)} · {count} / {total} 次调用
      </div>
      {sourceLabel && (
        <div className="mt-1 text-xs text-gray-400 dark:text-gray-500">来源：{sourceLabel}</div>
      )}
      <p className="mt-2 text-xs text-gray-400 dark:text-gray-500">{hint}</p>
    </div>
  )
}

function formatTimingSources(sources) {
  if (!sources?.length) return '当前范围无可用样本'
  return sources.map((item) => `${item.source}（${item.quality}，${item.count}）`).join('、')
}

function RankingCard({ title, items, onClick }) {
  const count = (items || []).length
  return (
    <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">{title}</h2>
        <span className="text-xs text-gray-400 dark:text-gray-500">共 {count} 项</span>
      </div>
      <RankingList items={items} valueKey="value" labelKey="name" format={fmtTokensShort} secondaryKey="cost" secondaryFormat={fmtUsd} limit={count || 1} onItemClick={onClick} />
    </div>
  )
}
