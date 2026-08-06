// 费用页：总费用 + 趋势 + 模型/Agent/节点排行 + 高费用会话 + 未配置价格。

import React, { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import MetricCard from '../../components/cards/MetricCard'
import TrendChart from '../../components/charts/TrendChart'
import RankingList from '../../components/cards/RankingList'
import { ErrorState, LoadingSkeleton, DataQualityNote } from '../../components/feedback/Feedback'
import { api, q, rangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { fmtUsd, fmtTokensShort } from '../../services/format'

export default function Costs() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const params = rangeParams(range)

  const overview = useQuery(`overview${q(params)}`, () => api(`/overview${q(params)}`))
  const series = useQuery(`ts${q(params)}`, () => api(`/usage/timeseries${q(params)}`))
  const byModel = useQuery(`bm${q({ ...params, dim: 'model' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'model' })}`))
  const byAgent = useQuery(`ba${q({ ...params, dim: 'client' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'client' })}`))
  const byNode = useQuery(`bn${q({ ...params, dim: 'node' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'node' })}`))
  const models = useQuery(`models${q(params)}`, () => api(`/models${q(params)}`))

  const trend = useMemo(() => ({
    labels: (series.data?.series || []).map((p) => p.bucket),
    values: (series.data?.series || []).map((p) => p.cost_micro_usd),
  }), [series.data])

  if (overview.error) return <ErrorState error={overview.error} onRetry={overview.refresh} />
  if (overview.loading) return <LoadingSkeleton rows={6} />
  const o = overview.data || {}

  // 按费用排序（用 model_calls 估算，因为 breakdown 未返回 cost）
  const rank = (arr) => (arr || []).map((m) => ({ id: m.dimension, name: m.dimension, value: m.model_calls ?? 0 }))

  const missingPricing = (models.data?.models || []).filter((m) => !m.pricing_source || m.pricing_source === 'builtin_catalog')

  return (
    <>
      <PageHeader title="费用" subtitle="费用分析与价格配置检查" />
      <DataQualityNote kind="estimated" text="费用区分：已确认费用 / 按 Token 估算 / 价格缺失无法计算。估算值不代表精确账单。" />

      <div className="mt-4 grid grid-cols-12 gap-6">
        <MetricCard label="总费用" value={fmtUsd(o.calculated_cost_micro_usd ?? o.estimated_cost_micro_usd)} sub="估算费用" hint="按 Token 与价格目录估算" />
        <MetricCard label="平均单会话费用" value={o.sessions > 0 ? fmtUsd((o.estimated_cost_micro_usd ?? 0) / o.sessions) : '—'} />
        <MetricCard label="平均单请求费用" value={o.model_calls > 0 ? fmtUsd((o.estimated_cost_micro_usd ?? 0) / o.model_calls) : '—'} />
        <MetricCard label="费用趋势" value={fmtUsd(o.estimated_cost_micro_usd)} sub="当前范围" />
      </div>

      <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">费用趋势</h2>
        <TrendChart labels={trend.labels} values={trend.values} height={340} formatY={fmtUsd} />
      </div>

      <div className="mt-6 grid grid-cols-1 xl:grid-cols-3 gap-6">
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">模型费用排行</h2>
          <RankingList items={rank(byModel.data?.by)} valueKey="value" labelKey="name" format={fmtTokensShort} limit={5} onItemClick={(m) => navigate(`/models/${encodeURIComponent(m.id)}`)} />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">Agent 费用排行</h2>
          <RankingList items={rank(byAgent.data?.by)} valueKey="value" labelKey="name" format={fmtTokensShort} limit={5} onItemClick={(a) => navigate(`/agents/${encodeURIComponent(a.id)}`)} />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">节点费用排行</h2>
          <RankingList items={rank(byNode.data?.by)} valueKey="value" labelKey="name" format={fmtTokensShort} limit={5} onItemClick={(n) => navigate(`/nodes/${encodeURIComponent(n.id)}`)} />
        </div>
      </div>

      {missingPricing.length > 0 && (
        <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-2">未配置价格的模型</h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">这些模型缺少价格，费用无法精确计算。</p>
          <div className="flex flex-wrap gap-2">
            {missingPricing.slice(0, 10).map((m) => (
              <button key={m.model} type="button" onClick={() => navigate(`/models/${encodeURIComponent(m.model)}`)} className="px-3 py-1.5 rounded-lg bg-amber-50 dark:bg-amber-400/10 text-amber-700 dark:text-amber-400 text-xs font-medium">
                {m.model}
              </button>
            ))}
          </div>
        </div>
      )}
    </>
  )
}
