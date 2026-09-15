// 费用页：总费用 + 趋势 + 模型/Agent/节点排行 + 高费用会话 + 未配置价格。

import React, { useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import MetricCard from '../../components/cards/MetricCard'
import TrendChart from '../../components/charts/TrendChart'
import RankingList from '../../components/cards/RankingList'
import { ErrorState, LoadingSkeleton, DataQualityNote } from '../../components/feedback/Feedback'
import { api, q, usageRangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { useNodeFilter } from '../../hooks/useNodeFilter'
import { useNodeNames } from '../../hooks/useNodeNames'
import { previousTimeRange } from '../../hooks/timeRangeState'
import { fmtChange, changeTone, fmtPct, fmtUsd } from '../../services/format'

export default function Costs() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const { nodeId, clientId, model, projectId } = useNodeFilter()
  const params = usageRangeParams(range, { nodeId, clientId, model, projectId })
  const previousRange = useMemo(() => previousTimeRange(range), [range])
  const previousParams = usageRangeParams(previousRange, { nodeId, clientId, model, projectId })

  const overview = useQuery(`overview${q(params)}`, () => api(`/overview${q(params)}`))
  const previousOverview = useQuery(`cost-overview-previous${q(previousParams)}`, () => api(`/overview${q(previousParams)}`), { enabled: Boolean(previousRange) })
  const series = useQuery(`ts${q(params)}`, () => api(`/usage/timeseries${q(params)}`))
  const byModel = useQuery(`bm${q({ ...params, dim: 'model' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'model' })}`))
  const byAgent = useQuery(`ba${q({ ...params, dim: 'client' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'client' })}`))
  const byNode = useQuery(`bn${q({ ...params, dim: 'node' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'node' })}`))
  const nodeNames = useNodeNames()

  const trend = useMemo(() => ({
    labels: (series.data?.series || []).map((p) => p.bucket),
    values: (series.data?.series || []).map((p) => p.cost_micro_usd),
  }), [series.data])

  if (overview.error) return <ErrorState error={overview.error} onRetry={overview.refresh} />
  if (overview.loading) return <LoadingSkeleton rows={6} />
  const o = overview.data || {}
  const previous = previousOverview.data || null
  const totalCost = (o.reported_cost_micro_usd ?? 0) + (o.calculated_cost_micro_usd ?? 0) + (o.estimated_cost_micro_usd ?? 0)
  const previousTotalCost = previous ? (previous.reported_cost_micro_usd ?? 0) + (previous.calculated_cost_micro_usd ?? 0) + (previous.estimated_cost_micro_usd ?? 0) : null
  const compare = (current, previousValue) => ({
    delta: previous ? fmtChange(current, previousValue) : null,
    deltaTone: previous ? changeTone(current, previousValue) : 'neutral',
  })
  const coverage = o.pricing_coverage || {}
  const costMode = (coverage.priced_calls ?? 0) > 0
    ? '客户端上报 / 规则计算 / 估算'
    : '当前范围没有可用价格'
  const costHint = (coverage.total_calls ?? 0) > 0
    ? `${coverage.priced_calls ?? 0}/${coverage.total_calls} 次调用有费用口径`
    : '当前范围没有模型调用'

  // 按费用排序（breakdown 返回 cost_micro_usd）
  const rank = (arr) => (arr || [])
    .filter((m) => m.cost_micro_usd != null)
    .map((m) => ({ id: m.dimension, name: nodeNames[m.dimension] || m.dimension, value: m.cost_micro_usd }))

  const missingPricing = coverage.unpriced_models || []

  return (
    <>
      <PageHeader title="费用" subtitle="费用分析与价格配置检查" />
      <DataQualityNote kind="estimated" text="费用口径：客户端上报 / 价格规则计算 / Token 估算；缺失价格不计入。估算值不代表精确账单。" />

      <div className="mt-4 grid grid-cols-12 gap-6">
        <MetricCard label="总费用" value={(coverage.priced_calls ?? 0) > 0 ? fmtUsd(totalCost) : '—'} {...compare(totalCost, previousTotalCost)} sub={costMode} hint={costHint} />
        <MetricCard label="平均单会话费用" value={o.sessions > 0 && (coverage.priced_calls ?? 0) > 0 ? fmtUsd(totalCost / o.sessions) : '—'} {...compare(o.sessions > 0 ? totalCost / o.sessions : null, previous?.sessions > 0 ? previousTotalCost / previous.sessions : null)} />
        <MetricCard label="平均已定价请求费用" value={(coverage.priced_calls ?? 0) > 0 ? fmtUsd(totalCost / coverage.priced_calls) : '—'} {...compare(coverage.priced_calls > 0 ? totalCost / coverage.priced_calls : null, previous?.pricing_coverage?.priced_calls > 0 ? previousTotalCost / previous.pricing_coverage.priced_calls : null)} />
        <MetricCard label="价格覆盖率" value={fmtPct(coverage.ratio)} {...compare(coverage.ratio, previous?.pricing_coverage?.ratio)} sub={`${coverage.priced_calls ?? 0} / ${coverage.total_calls ?? 0} 次调用`} />
      </div>

      <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">费用趋势</h2>
        <TrendChart labels={trend.labels} values={trend.values} range={range} height={320} formatY={fmtUsd} />
      </div>

      <div className="mt-4 grid grid-cols-1 xl:grid-cols-3 gap-6">
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">模型费用排行</h2>
          <RankingList items={rank(byModel.data?.by)} valueKey="value" labelKey="name" format={fmtUsd} limit={5} onItemClick={(m) => navigate(`/models/${encodeURIComponent(m.id)}`)} />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">Agent 费用排行</h2>
          <RankingList items={rank(byAgent.data?.by)} valueKey="value" labelKey="name" format={fmtUsd} limit={5} onItemClick={(a) => navigate(`/agents/${encodeURIComponent(a.id)}`)} />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">节点费用排行</h2>
          <RankingList items={rank(byNode.data?.by)} valueKey="value" labelKey="name" format={fmtUsd} limit={5} onItemClick={(n) => navigate(`/nodes/${encodeURIComponent(n.id)}`)} />
        </div>
      </div>

      {missingPricing.length > 0 && (
        <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-2">未配置价格的模型</h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">这些调用没有可追溯价格，未计入总费用，也不会按 0 美元处理。</p>
          <div className="flex flex-wrap gap-2">
            {missingPricing.slice(0, 10).map((m) => (
              <button key={`${m.provider}/${m.model}`} type="button" onClick={() => navigate(`/models/${encodeURIComponent(m.model)}`)} className="px-3 py-1.5 rounded-lg bg-amber-50 dark:bg-amber-400/10 text-amber-700 dark:text-amber-400 text-xs font-medium">
                {m.model} · {m.calls} 次
              </button>
            ))}
          </div>
        </div>
      )}
    </>
  )
}
