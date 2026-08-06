// 会话列表：默认列 开始时间/Agent/模型/节点/持续时间/Token/费用/状态。

import React, { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DataTable from '../../components/tables/DataTable'
import StatusBadge from '../../components/common/StatusBadge'
import FilterBar from '../../components/filters/FilterBar'
import { ErrorState, LoadingSkeleton } from '../../components/feedback/Feedback'
import { api, q, rangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { fmtDateTime, fmtDuration, fmtTokensShort, fmtUsd, fmtBytes } from '../../services/format'

export default function Sessions() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const params = rangeParams(range)
  const [search, setSearch] = useState('')

  const query = useQuery(`sessions-list${q({ ...params, limit: 200 })}`, () => api(`/sessions${q({ ...params, limit: 200 })}`))

  const filtered = useMemo(() => {
    const list = query.data?.sessions || []
    if (!search) return list
    const s = search.toLowerCase()
    return list.filter((x) =>
      (x.source_session_id || '').toLowerCase().includes(s) ||
      (x.title || '').toLowerCase().includes(s) ||
      (x.client_id || '').toLowerCase().includes(s)
    )
  }, [query.data, search])

  if (query.error) return <ErrorState error={query.error} onRetry={query.refresh} />
  if (query.loading) return <LoadingSkeleton rows={8} />

  const columns = [
    { key: 'started_at', label: '开始时间', sortable: true, render: (r) => fmtDateTime(r.started_at) },
    { key: 'client_id', label: 'Agent', sortable: true, render: (r) => r.client_id || '—' },
    { key: 'model', label: '模型', sortable: true, render: (r) => r.model || '—' },
    { key: 'node_id', label: '节点', sortable: true, render: (r) => r.node_id || '—' },
    { key: 'duration', label: '持续时间', render: (r) => fmtDuration(r.duration_ms) },
    { key: 'input_tokens', label: 'Token', sortable: true, render: (r) => fmtTokensShort((r.input_tokens ?? 0) + (r.output_tokens ?? 0)) },
    { key: 'calculated_cost_micro_usd', label: '费用', sortable: true, render: (r) => fmtUsd(r.calculated_cost_micro_usd ?? r.estimated_cost_micro_usd) },
    { key: 'status', label: '状态', render: (r) => <StatusBadge status={r.status} /> },
  ]

  return (
    <>
      <PageHeader title="会话" subtitle="查看所有 Agent 会话的 Token、费用与流量" />
      <FilterBar searchPlaceholder="搜索会话标题或 ID…" onSearch={setSearch} />
      <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-4">
        <DataTable
          columns={columns}
          data={filtered}
          pageSize={15}
          onRowClick={(r) => navigate(`/sessions/${encodeURIComponent(r.id)}`)}
        />
      </div>
    </>
  )
}
