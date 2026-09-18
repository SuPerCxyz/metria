// Agents 列表：Agent 名称/请求数/会话数/Token/费用。

import React, { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DataTable from '../../components/tables/DataTable'
import FilterBar from '../../components/filters/FilterBar'
import { ErrorState, LoadingSkeleton } from '../../components/feedback/Feedback'
import { api, q, usageRangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { useNodeFilter } from '../../hooks/useNodeFilter'
import { fmtTokensShort, fmtUsd, fmtPct100, sumTokens, cacheHitRate } from '../../services/format'

const AGENT_LABELS = {
  'claude-code': 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
}

export default function Agents() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const { nodeId, clientId, model, projectId } = useNodeFilter()
  const params = usageRangeParams(range, { nodeId, clientId, model, projectId })
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
    { key: 'dimension', label: 'Agent 名称', sortValue: (r) => AGENT_LABELS[r.dimension] || r.dimension, render: (r) => AGENT_LABELS[r.dimension] || r.dimension },
    { key: 'input_tokens', label: 'Token', sortValue: (r) => sumTokens(r), render: (r) => fmtTokensShort(sumTokens(r)) },
    { key: 'cache', label: '缓存命中率', hideWhenEmpty: true, sortValue: (r) => cacheHitRate(r), render: (r) => cacheHitRate(r) != null ? fmtPct100(cacheHitRate(r)) : '—' },
    { key: 'model_calls', label: '请求数', sortable: true, render: (r) => String(r.model_calls ?? 0) },
    { key: 'sessions', label: '会话数', render: (r) => String(r.sessions ?? 0) },
    { key: 'cost', label: '费用', hideWhenEmpty: true, sortValue: (r) => r.cost_micro_usd, render: (r) => fmtUsd(r.cost_micro_usd) },
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
