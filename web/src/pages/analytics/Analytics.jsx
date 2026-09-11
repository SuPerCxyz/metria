// 使用分析：Token / 请求 / 缓存 / 延迟 标签切换。

import React, { useCallback, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import MetricCard from '../../components/cards/MetricCard'
import TrendChart from '../../components/charts/TrendChart'
import RankingList from '../../components/cards/RankingList'
import Segmented from '../../components/ui/Segmented'
import { ErrorState, LoadingSkeleton, EmptyState } from '../../components/feedback/Feedback'
import { api, q, rangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { useNodeFilter } from '../../hooks/useNodeFilter'
import { useNodeNames } from '../../hooks/useNodeNames'
import { fmtTokensShort, fmtUsd, fmtBytes, fmtPct, fmtPct100, fmtDuration, sumTokens } from '../../services/format'

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
  const params = rangeParams(range)
  const { nodeId } = useNodeFilter()
  if (nodeId) params.node_id = nodeId
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

  const overview = useQuery(`overview${q(scopedParams)}`, () => api(`/overview${q(scopedParams)}`))
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
  const nodeNames = useNodeNames()

  const latencyTrend = useMemo(() => {
    const pts = latencySeries.data?.series || []
    const labels = pts.map((p) => p.bucket)
    const pick = (key) => pts.map((p) => p[key] ?? null)
    return {
      labels,
      datasets: [
        { label: 'P50', values: pick('p50_ms'), color: '#10b981' },
        { label: 'P95', values: pick('p95_ms'), color: '#f59e0b' },
        { label: '平均', values: pick('avg_ms'), color: '#6366f1' },
      ],
    }
  }, [latencySeries.data])

  const trendData = useMemo(() => {
    const pts = series.data?.series || []
    switch (trendTab) {
      case 'tokens': return { labels: pts.map((p) => p.bucket), values: pts.map((p) => (p.input_tokens || 0) + (p.output_tokens || 0) + (p.cache_read_tokens || 0)) }
      case 'cost': return { labels: pts.map((p) => p.bucket), values: pts.map((p) => p.cost_micro_usd) }
      case 'traffic': return { labels: pts.map((p) => p.bucket), values: pts.map((p) => p.estimated_traffic_bytes) }
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
        case 'traffic': return point?.estimated_traffic_bytes ?? 0
        default: return (point?.input_tokens ?? 0) + (point?.output_tokens ?? 0) + (point?.cache_read_tokens ?? 0)
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

  const cacheHitRate = o.input_tokens > 0 ? o.cache_read_tokens / (o.input_tokens + o.cache_read_tokens) : 0

  const modelItems = (byModel.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '' && m.dimension !== '(unknown)')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: sumTokens(m) }))
  const agentItems = (byAgent.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '')
    .map((m) => ({ id: m.dimension, name: m.dimension, value: sumTokens(m) }))
  const nodeItems = (byNode.data?.by || [])
    .filter((m) => m.dimension && m.dimension !== '')
    .map((m) => ({ id: m.dimension, name: nodeNames[m.dimension] || m.dimension, value: sumTokens(m) }))

  return (
    <>
      <PageHeader title="使用分析" subtitle="深入分析 Token、请求、缓存与延迟" />
      <div className="mb-3">
        <Segmented items={TABS} value={tab} onChange={setTab} />
      </div>

      {tab === 'tokens' && (
        <>
          <div className="grid grid-cols-12 gap-6">
            <MetricCard label="总 Token" value={fmtTokensShort(sumTokens(o))} sub={`输入 ${fmtTokensShort(o.input_tokens)} · 输出 ${fmtTokensShort(o.output_tokens)}`} />
            <MetricCard label="输入 Token" value={fmtTokensShort(o.input_tokens)} sub="请求上下文" />
            <MetricCard label="输出 Token" value={fmtTokensShort(o.output_tokens)} sub="模型生成" />
            <MetricCard label="缓存 Token" value={fmtTokensShort(o.cache_read_tokens)} sub="缓存读取" />
          </div>
          <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
            <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">Token 趋势</h2>
            <TrendChart labels={trendData.labels} values={trendData.values} height={320} formatY={fmtTokensShort} />
          </div>
          <div className="mt-4 grid grid-cols-1 xl:grid-cols-3 gap-6">
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
            <MetricCard label="成功请求" value={fmtTokensShort((o.model_calls ?? 0) - (o.failed_calls ?? 0))} sub={`失败 ${o.failed_calls ?? 0} 次`} />
            <MetricCard label="失败请求" value={fmtTokensShort(o.failed_calls ?? 0)} sub="状态非成功" />
            <MetricCard label="平均请求频率" value={o.sessions > 0 ? ((o.model_calls ?? 0) / o.sessions).toFixed(1) : '—'} sub="每会话请求数" />
          </div>
          <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
            <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">请求趋势</h2>
            <TrendChart labels={trendData.labels} values={trendData.values} height={320} formatY={(v) => v.toLocaleString()} />
          </div>
        </>
      )}

      {tab === 'cache' && (
        <>
          <div className="grid grid-cols-12 gap-6">
            <MetricCard label="缓存 Token" value={fmtTokensShort(o.cache_read_tokens)} sub="缓存读取" />
            <MetricCard label="缓存命中率" value={fmtPct100(cacheHitRate)} />
            <MetricCard label="缓存节省费用" value={o.cache_savings_micro_usd > 0 ? fmtUsd(o.cache_savings_micro_usd) : '—'} sub={o.cache_savings_micro_usd > 0 ? '按缓存读取单价估算' : '无价格规则或缓存数据'} />
            <MetricCard label="缓存写入" value={fmtTokensShort(o.cache_write_tokens)} />
          </div>
          <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
            <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">缓存趋势</h2>
            <TrendChart labels={trendData.labels} values={trendData.values} height={320} formatY={fmtTokensShort} />
          </div>
        </>
      )}

      {detailMode && (
        <DetailAnalysis
          tab={tab}
          metric={detailMetric}
          onMetricChange={setDetailMetric}
          overview={o}
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
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">延迟分析</h2>
          {performance.loading ? <LoadingSkeleton rows={2} /> : performance.error ? (
            <ErrorState error={performance.error} onRetry={performance.refresh} />
          ) : (
            <>
              <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                <PerformanceCard
                  label="首个可观察输出延迟"
                  value={performance.data?.ttft?.avg_ms != null ? fmtDuration(performance.data.ttft.avg_ms) : '不可用'}
                  count={performance.data?.ttft?.count}
                  total={performance.data?.total_calls}
                  coverage={performance.data?.ttft?.coverage}
                  hint="从客户端只读日志的请求起点到首个可观察输出事件，不等同服务端精确首 Token。"
                />
                <PerformanceCard
                  label="估算输出速度"
                  value={performance.data?.output_speed?.avg_tokens_per_second != null ? `${performance.data.output_speed.avg_tokens_per_second.toFixed(1)} Token/s` : '不可用'}
                  count={performance.data?.output_speed?.count}
                  total={performance.data?.total_calls}
                  coverage={performance.data?.output_speed?.coverage}
                  hint="仅在首输出、完成时间和输出 Token 均可用时计算。"
                />
              </div>
              <p className="mt-3 text-xs text-gray-400 dark:text-gray-500">
                观测来源：{formatTimingSources(performance.data?.sources)}；不支持的调用保持不可用，不按 0 计入。
              </p>
            </>
          )}

          <h3 className="mb-3 mt-6 text-sm font-semibold text-gray-500 dark:text-gray-400">端到端调用时长</h3>
          {latency.loading ? <LoadingSkeleton rows={3} /> : latency.error ? (
            <ErrorState error={latency.error} onRetry={latency.refresh} />
          ) : (latency.data?.count ?? 0) > 0 ? (
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
                    height={300}
                    formatY={fmtDuration}
                    ariaLabel="延迟趋势，P50、P95 与平均时长随时间变化"
                  />
                ) : (
                  <EmptyState title="暂无延迟数据" desc="所选范围内客户端日志未记录调用时长（duration_ms 缺失）。" />
                )}
              </div>
            </>
          ) : (
            <EmptyState title="暂无延迟数据" desc="客户端日志未记录调用时长（duration_ms 缺失），暂无法计算 P50/P95/P99。" />
          )}
        </div>
      )}
    </>
  )
}

const DETAIL_COLORS = ['#6366f1', '#10b981', '#f59e0b', '#ef4444', '#06b6d4', '#8b5cf6', '#ec4899', '#84cc16']

function DetailAnalysis({
  tab,
  metric,
  onMetricChange,
  overview,
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
    { key: 'traffic', label: '估算流量' },
  ]
  const formatY = metric === 'cost'
    ? fmtUsd
    : metric === 'traffic'
      ? fmtBytes
      : metric === 'requests'
        ? (value) => value.toLocaleString()
        : fmtTokensShort
  const title = tab === 'agent-detail' ? '按 Agent 详细分析' : '按模型详细分析'
  const chartTitle = tab === 'agent-detail' ? 'Agent 趋势对比' : '模型趋势对比'

  return (
    <>
      <div className="grid grid-cols-12 gap-6">
        <MetricCard label="范围 Token" value={fmtTokensShort(sumTokens(overview))} sub={`请求 ${fmtTokensShort(overview.model_calls)}`} />
        <MetricCard label="请求数" value={fmtTokensShort(overview.model_calls)} sub={`会话 ${fmtTokensShort(overview.sessions)}`} />
        <MetricCard label="费用" value={fmtUsd(overview.calculated_cost_micro_usd ?? overview.estimated_cost_micro_usd)} sub="当前范围" />
        <MetricCard label="平均延迟" value={latency?.avg_ms != null ? fmtDuration(latency.avg_ms) : '—'} sub="有记录的请求" />
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

function PerformanceCard({ label, value, count = 0, total = 0, coverage, hint }) {
  return (
    <div className="rounded-xl bg-gray-50 p-4 dark:bg-gray-700/30">
      <div className="text-xs text-gray-400 dark:text-gray-500">{label}</div>
      <div className="mt-1 text-xl font-bold tabular-nums text-gray-800 dark:text-gray-100">{value}</div>
      <div className="mt-1 text-xs text-gray-500 dark:text-gray-400">
        覆盖率 {fmtPct(coverage)} · {count} / {total} 次调用
      </div>
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
      <RankingList items={items} valueKey="value" labelKey="name" format={fmtTokensShort} limit={count || 1} onItemClick={onClick} />
    </div>
  )
}
