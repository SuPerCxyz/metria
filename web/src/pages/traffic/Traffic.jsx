// 网络流量页：总流量 + 趋势 + 排行 + 高流量会话。标记精确/估算。

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
import { fmtBytes, fmtTokensShort } from '../../services/format'

export default function Traffic() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const params = rangeParams(range)

  const overview = useQuery(`overview${q(params)}`, () => api(`/overview${q(params)}`))
  const series = useQuery(`ts${q(params)}`, () => api(`/usage/timeseries${q(params)}`))
  const byModel = useQuery(`tm${q({ ...params, dim: 'model' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'model' })}`))
  const byAgent = useQuery(`ta${q({ ...params, dim: 'client' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'client' })}`))
  const byNode = useQuery(`tn${q({ ...params, dim: 'node' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'node' })}`))

  const trend = useMemo(() => ({
    labels: (series.data?.series || []).map((p) => p.bucket),
    values: (series.data?.series || []).map((p) => p.estimated_traffic_bytes),
  }), [series.data])

  if (overview.error) return <ErrorState error={overview.error} onRetry={overview.refresh} />
  if (overview.loading) return <LoadingSkeleton rows={6} />
  const o = overview.data || {}

  const rank = (arr) => (arr || []).map((m) => ({ id: m.dimension, name: m.dimension, value: m.estimated_traffic_bytes ?? 0 }))

  return (
    <>
      <PageHeader title="网络流量" subtitle="请求与响应流量分析" />
      <DataQualityNote kind="estimated" text="网络流量来自精确统计或估算，标记为估算值。不代表网卡真实流量或云厂商计费流量。" />

      <div className="mt-4 grid grid-cols-12 gap-6">
        <MetricCard label="总流量" value={fmtBytes(o.estimated_total_bytes)} sub="估算流量" hint={`范围 ${fmtBytes(o.traffic_lower_bound_bytes)} ~ ${fmtBytes(o.traffic_upper_bound_bytes)}`} />
        <MetricCard label="请求流量" value={fmtBytes(o.estimated_request_bytes)} sub="估算" />
        <MetricCard label="响应流量" value={fmtBytes(o.estimated_response_bytes)} sub="估算" />
        <MetricCard label="平均单会话流量" value={o.sessions > 0 ? fmtBytes((o.estimated_total_bytes ?? 0) / o.sessions) : '—'} />
      </div>

      <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">流量趋势</h2>
        <TrendChart labels={trend.labels} values={trend.values} height={340} formatY={fmtBytes} />
      </div>

      <div className="mt-6 grid grid-cols-1 xl:grid-cols-3 gap-6">
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">Agent 流量排行</h2>
          <RankingList items={rank(byAgent.data?.by)} valueKey="value" labelKey="name" format={fmtBytes} limit={5} onItemClick={(a) => navigate(`/agents/${encodeURIComponent(a.id)}`)} />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">模型流量排行</h2>
          <RankingList items={rank(byModel.data?.by)} valueKey="value" labelKey="name" format={fmtBytes} limit={5} onItemClick={(m) => navigate(`/models/${encodeURIComponent(m.id)}`)} />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">节点流量排行</h2>
          <RankingList items={rank(byNode.data?.by)} valueKey="value" labelKey="name" format={fmtBytes} limit={5} onItemClick={(n) => navigate(`/nodes/${encodeURIComponent(n.id)}`)} />
        </div>
      </div>
    </>
  )
}
