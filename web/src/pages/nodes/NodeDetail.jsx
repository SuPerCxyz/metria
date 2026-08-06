// 节点详情：摘要 + 趋势 + 节点上的 Agent + 使用的模型 + 最近会话 + 上报状态。

import React, { useMemo } from 'react'
import { Link, useParams, useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DetailSummary from '../../components/common/DetailSummary'
import StatusBadge from '../../components/common/StatusBadge'
import TrendChart from '../../components/charts/TrendChart'
import RankingList from '../../components/cards/RankingList'
import { ErrorState, LoadingSkeleton, EmptyState } from '../../components/feedback/Feedback'
import { api, q, rangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { fmtTokensShort, fmtUsd, fmtBytes, fmtDateTime, fmtRelative } from '../../services/format'

export default function NodeDetail() {
  const { id } = useParams()
  const navigate = useNavigate()
  const { range } = useTimeRange()
  const params = rangeParams(range)

  const query = useQuery(`node-detail-${id}`, () => api(`/nodes/${encodeURIComponent(id)}${q(params)}`))
  const series = useQuery(`node-ts-${id}${q(params)}`, () => api(`/usage/timeseries${q({ ...params, node_id: id })}`))

  const trend = useMemo(() => ({
    labels: (series.data?.series || []).map((p) => p.bucket),
    values: (series.data?.series || []).map((p) => p.input_tokens + p.output_tokens),
  }), [series.data])

  if (query.error) return <ErrorState error={query.error} />
  if (query.loading) return <LoadingSkeleton rows={8} />
  const n = query.data?.node || {}
  const rs = query.data?.range_summary || {}

  const isOnline = n.status === 'online' || n.status === 'active'

  return (
    <>
      <PageHeader
        back={<Link to="/nodes" className="text-sm text-indigo-600 dark:text-indigo-400 hover:underline">← 返回节点</Link>}
        title={n.name || id}
        subtitle={<span>{n.id} · <StatusBadge status={n.status} /></span>}
      />

      {!isOnline && (
        <div className="mb-6 px-4 py-3 rounded-xl border border-amber-200 dark:border-amber-800 bg-amber-50 dark:bg-amber-400/5 text-sm text-amber-700 dark:text-amber-400">
          节点离线或停止上报，最近上报：{fmtRelative(n.last_seen_at)}
        </div>
      )}

      <DetailSummary
        items={[
          { label: '平台', value: `${n.platform || '—'} / ${n.architecture || '—'}` },
          { label: '状态', value: n.status || '—' },
          { label: 'Agent 数量', value: String((query.data?.collectors || []).length) },
          { label: '会话数', value: String(rs.sessions ?? '—') },
          { label: 'Token', value: fmtTokensShort((rs.input_tokens ?? 0) + (rs.output_tokens ?? 0)) },
          { label: '费用', value: fmtUsd(rs.cost_micro_usd) },
          { label: '网络流量', value: fmtBytes(rs.estimated_total_bytes) },
          { label: '最后上报', value: fmtDateTime(n.last_seen_at) },
        ]}
      />

      <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">Token / 费用 / 流量趋势</h2>
        {trend.labels.length === 0 ? <EmptyState title="当前范围无数据" /> : <TrendChart labels={trend.labels} values={trend.values} height={320} formatY={fmtTokensShort} />}
      </div>

      <div className="mt-6 grid grid-cols-1 xl:grid-cols-2 gap-6">
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">节点上的 Agent</h2>
          {(query.data?.collectors || []).length === 0 && <EmptyState title="暂无 Agent" />}
          <div className="space-y-2">
            {(query.data?.collectors || []).map((c) => (
              <div key={c.id} className="flex items-center justify-between px-3 py-2 rounded-lg bg-gray-50 dark:bg-gray-700/30">
                <div>
                  <div className="text-sm font-medium text-gray-700 dark:text-gray-200">{c.id}</div>
                  <div className="text-xs text-gray-400 dark:text-gray-500">v{c.agent_version} · 协议 {c.protocol_version}</div>
                </div>
                <StatusBadge status={c.status} />
              </div>
            ))}
          </div>
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">使用的模型</h2>
          <RankingList items={(query.data?.by_model || []).map((m) => ({ id: m.model, name: m.model, value: m.calls ?? 0 }))} valueKey="value" labelKey="name" format={fmtTokensShort} limit={6} onItemClick={(m) => navigate(`/models/${encodeURIComponent(m.id)}`)} />
        </div>
      </div>

      <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">最近会话</h2>
        <div className="space-y-2">
          {(query.data?.recent_sessions || query.data?.sessions || []).slice(0, 8).length === 0 && <EmptyState title="暂无会话" />}
          {(query.data?.recent_sessions || query.data?.sessions || []).slice(0, 8).map((s) => (
            <button key={s.id} type="button" onClick={() => navigate(`/sessions/${encodeURIComponent(s.id)}`)} className="w-full flex items-center justify-between px-3 py-2 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700/30 text-left">
              <span className="text-sm text-gray-700 dark:text-gray-200 truncate">{s.title || s.source_session_id}</span>
              <span className="text-xs text-gray-400 dark:text-gray-500 tabular-nums">{fmtDateTime(s.started_at)} · {fmtTokensShort(s.input_tokens)} tokens</span>
            </button>
          ))}
        </div>
      </div>
    </>
  )
}
