// Agent 详情：使用趋势 + 模型分布 + 节点分布 + 最近会话 + 版本分布。

import React, { useMemo } from 'react'
import { Link, useParams, useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DetailSummary from '../../components/common/DetailSummary'
import TrendChart from '../../components/charts/TrendChart'
import RankingList from '../../components/cards/RankingList'
import { ErrorState, LoadingSkeleton, EmptyState } from '../../components/feedback/Feedback'
import { api, q, rangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { fmtTokensShort, fmtUsd, fmtBytes, fmtDateTime } from '../../services/format'

const AGENT_LABELS = {
  'claude-code': 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
}

export default function AgentDetail() {
  const { id } = useParams()
  const navigate = useNavigate()
  const { range } = useTimeRange()
  const params = rangeParams(range)

  const query = useQuery(`agent-detail-${id}${q(params)}`, () => api(`/clients/${encodeURIComponent(id)}${q(params)}`))
  const models = useQuery(`agent-models-${id}`, () => api(`/clients/${encodeURIComponent(id)}/models`))
  const series = useQuery(`agent-ts-${id}${q(params)}`, () => api(`/usage/timeseries${q({ ...params, client_id: id })}`))

  const trend = useMemo(() => ({
    labels: (series.data?.series || []).map((p) => p.bucket),
    values: (series.data?.series || []).map((p) => p.input_tokens + p.output_tokens),
  }), [series.data])

  if (query.error) return <ErrorState error={query.error} />
  if (query.loading) return <LoadingSkeleton rows={8} />
  const d = query.data || {}

  return (
    <>
      <PageHeader
        back={<Link to="/agents" className="text-sm text-indigo-600 dark:text-indigo-400 hover:underline">← 返回 Agents</Link>}
        title={AGENT_LABELS[id] || id}
        subtitle={id}
      />

      <DetailSummary
        items={[
          { label: '费用', value: fmtUsd(d.calculated_cost_micro_usd) },
          { label: '网络流量', value: fmtBytes(d.estimated_total_bytes) },
          { label: 'Source 健康', value: d.source_health ? `${d.source_health.healthy ?? 0}/${d.source_health.total ?? 0}` : '—' },
          { label: '版本数', value: String((d.version_dist || []).length) },
        ]}
      />

      <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">使用趋势</h2>
        {trend.labels.length === 0 ? <EmptyState title="当前范围无数据" /> : <TrendChart labels={trend.labels} values={trend.values} height={320} formatY={fmtTokensShort} />}
      </div>

      <div className="mt-6 grid grid-cols-1 xl:grid-cols-2 gap-6">
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">模型分布</h2>
          <RankingList items={(models.data?.models || []).map((m) => ({ id: m.model, name: m.model, value: m.calls ?? 0 }))} valueKey="value" labelKey="name" format={fmtTokensShort} limit={6} onItemClick={(m) => navigate(`/models/${encodeURIComponent(m.id)}`)} />
        </div>
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">节点分布</h2>
          <RankingList items={(d.by_node || []).map((n) => ({ id: n.node_id, name: n.node_id, value: n.model_calls ?? 0 }))} valueKey="value" labelKey="name" format={fmtTokensShort} limit={6} onItemClick={(n) => navigate(`/nodes/${encodeURIComponent(n.id)}`)} />
        </div>
      </div>

      {d.version_dist?.length > 0 && (
        <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">版本分布</h2>
          <div className="flex flex-wrap gap-2">
            {d.version_dist.map((v) => (
              <span key={v.version} className="px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-700/40 text-xs font-medium text-gray-600 dark:text-gray-300">
                v{v.version} · {v.count}
              </span>
            ))}
          </div>
        </div>
      )}

      <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">最近会话</h2>
        <div className="space-y-2">
          {(d.recent_sessions || []).length === 0 && <EmptyState title="暂无会话" />}
          {(d.recent_sessions || []).slice(0, 8).map((s) => (
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
