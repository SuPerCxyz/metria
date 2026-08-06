// Agents 列表：Agent 名称/客户端数/活跃节点/会话数/Token/费用/流量。

import React, { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DataTable from '../../components/tables/DataTable'
import FilterBar from '../../components/filters/FilterBar'
import { ErrorState, LoadingSkeleton } from '../../components/feedback/Feedback'
import { api, q, rangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { fmtTokensShort, fmtUsd, fmtBytes } from '../../services/format'

const AGENT_LABELS = {
  'claude-code': 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
}

export default function Agents() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const params = rangeParams(range)
  const [search, setSearch] = useState('')

  const query = useQuery(`agents${q({ ...params, dim: 'client' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'client' })}`))

  const filtered = useMemo(() => {
    const list = query.data?.by || []
    if (!search) return list
    const s = search.toLowerCase()
    return list.filter((x) => x.dimension.toLowerCase().includes(s))
  }, [query.data, search])

  if (query.error) return <ErrorState error={query.error} onRetry={query.refresh} />
  if (query.loading) return <LoadingSkeleton rows={5} />

  const columns = [
    { key: 'dimension', label: 'Agent 名称', sortable: true, render: (r) => AGENT_LABELS[r.dimension] || r.dimension },
    { key: 'input_tokens', label: 'Token', sortable: true, render: (r) => fmtTokensShort((r.input_tokens ?? 0) + (r.output_tokens ?? 0)) },
    { key: 'model_calls', label: '请求数', sortable: true, render: (r) => String(r.model_calls ?? 0) },
    { key: 'sessions', label: '会话数', render: (r) => String(r.sessions ?? 0) },
    { key: 'cost', label: '费用', render: (r) => fmtUsd(r.calculated_cost_micro_usd ?? r.estimated_cost_micro_usd) },
    { key: 'traffic', label: '网络流量', render: (r) => fmtBytes(r.estimated_traffic_bytes) },
  ]

  return (
    <>
      <PageHeader title="Agents" subtitle="不同编程 Agent 的使用情况对比" />
      <FilterBar searchPlaceholder="搜索 Agent…" onSearch={setSearch} />
      <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-4">
        <DataTable columns={columns} data={filtered} pageSize={12} onRowClick={(r) => navigate(`/agents/${encodeURIComponent(r.dimension)}`)} />
      </div>
    </>
  )
}
