// 估算流量页：总量 + 趋势 + 排行 + 覆盖率，始终标明非实际网卡流量。

import React, { useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import MetricCard from '../../components/cards/MetricCard'
import TrendChart from '../../components/charts/TrendChart'
import RankingList from '../../components/cards/RankingList'
import { ErrorState, LoadingSkeleton, DataQualityNote } from '../../components/feedback/Feedback'
import { api, q, rangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { useNodeFilter } from '../../hooks/useNodeFilter'
import { useNodeNames } from '../../hooks/useNodeNames'
import { fmtBytes, fmtPct } from '../../services/format'

export default function Traffic() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const params = rangeParams(range)
  const { nodeId } = useNodeFilter()
  if (nodeId) params.node_id = nodeId

  const overview = useQuery(`overview${q(params)}`, () => api(`/overview${q(params)}`))
  const series = useQuery(`ts${q(params)}`, () => api(`/usage/timeseries${q(params)}`))
  const byModel = useQuery(`tm${q({ ...params, dim: 'model' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'model' })}`))
  const byAgent = useQuery(`ta${q({ ...params, dim: 'client' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'client' })}`))
  const byNode = useQuery(`tn${q({ ...params, dim: 'node' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'node' })}`))
  const nodeNames = useNodeNames()

  const trend = useMemo(() => ({
    labels: (series.data?.series || []).map((p) => p.bucket),
    values: (series.data?.series || []).map((p) => p.estimated_traffic_bytes),
  }), [series.data])

  if (overview.error) return <ErrorState error={overview.error} onRetry={overview.refresh} />
  if (overview.loading) return <LoadingSkeleton rows={6} />
  const o = overview.data || {}
  const coverage = o.traffic_coverage || {}

  const rank = (arr) => (arr || []).map((m) => ({ id: m.dimension, name: nodeNames[m.dimension] || m.dimension, value: m.estimated_traffic_bytes ?? 0 }))

  return (
    <>
      <PageHeader title="估算流量" subtitle="按客户端日志与版本化 Profile 估算请求和响应传输量" />
      <DataQualityNote kind="estimated" text="全部数值均为估算流量及其区间，不代表实际网卡流量、精确传输量或云厂商计费流量。" />

      <div className="mt-4 grid grid-cols-12 gap-6">
        <MetricCard label="总估算流量" value={(coverage.estimated_calls ?? 0) > 0 ? fmtBytes(o.estimated_total_bytes) : '—'} sub="估算流量" hint={`估算区间 ${fmtBytes(o.traffic_lower_bound_bytes)} ~ ${fmtBytes(o.traffic_upper_bound_bytes)}`} />
        <MetricCard label="请求估算流量" value={(coverage.estimated_calls ?? 0) > 0 ? fmtBytes(o.estimated_request_bytes) : '—'} sub="估算" />
        <MetricCard label="响应估算流量" value={(coverage.estimated_calls ?? 0) > 0 ? fmtBytes(o.estimated_response_bytes) : '—'} sub="估算" />
        <MetricCard label="估算覆盖率" value={fmtPct(coverage.ratio)} sub={`${coverage.estimated_calls ?? 0} / ${coverage.total_calls ?? 0} 次调用`} hint={`${coverage.unavailable_calls ?? 0} 次调用缺少足够数据`} />
      </div>

      <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">估算流量趋势</h2>
        <TrendChart labels={trend.labels} values={trend.values} height={320} formatY={fmtBytes} />
      </div>

      <div className="mt-4 grid grid-cols-1 xl:grid-cols-3 gap-6">
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">Agent 估算流量排行</h2>
          <RankingList items={rank(byAgent.data?.by)} valueKey="value" labelKey="name" format={fmtBytes} limit={5} onItemClick={(a) => navigate(`/agents/${encodeURIComponent(a.id)}`)} />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">模型估算流量排行</h2>
          <RankingList items={rank(byModel.data?.by)} valueKey="value" labelKey="name" format={fmtBytes} limit={5} onItemClick={(m) => navigate(`/models/${encodeURIComponent(m.id)}`)} />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">节点估算流量排行</h2>
          <RankingList items={rank(byNode.data?.by)} valueKey="value" labelKey="name" format={fmtBytes} limit={5} onItemClick={(n) => navigate(`/nodes/${encodeURIComponent(n.id)}`)} />
        </div>
      </div>
    </>
  )
}
