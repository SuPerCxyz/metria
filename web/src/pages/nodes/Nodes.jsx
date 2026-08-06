// 节点列表：节点名称/状态/Agent数/会话数/Token/费用/流量/最后上报。

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
import { fmtDateTime, fmtTokensShort, fmtUsd, fmtBytes, fmtRelative } from '../../services/format'

export default function Nodes() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const params = rangeParams(range)
  const [search, setSearch] = useState('')

  const query = useQuery('nodes-list', () => api('/nodes'))
  const usage = useQuery(`nodes-usage${q({ ...params, dim: 'node' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'node' })}`))

  const usageMap = useMemo(() => {
    const m = new Map()
    for (const row of usage.data?.by || []) m.set(row.dimension, row)
    return m
  }, [usage.data])

  const filtered = useMemo(() => {
    const list = query.data?.nodes || []
    if (!search) return list
    const s = search.toLowerCase()
    return list.filter((x) => (x.name || '').toLowerCase().includes(s) || (x.id || '').toLowerCase().includes(s))
  }, [query.data, search])

  if (query.error) return <ErrorState error={query.error} onRetry={query.refresh} />
  if (query.loading) return <LoadingSkeleton rows={6} />

  const columns = [
    { key: 'name', label: '节点名称', sortable: true, render: (r) => r.name || r.id },
    { key: 'status', label: '状态', render: (r) => <StatusBadge status={r.status} /> },
    { key: 'collector_count', label: 'Agent 数量', render: (r) => String(r.collector_count ?? r.detected_clients ?? '—') },
    { key: 'sessions', label: '会话数', render: (r) => String(usageMap.get(r.id)?.sessions ?? '—') },
    { key: 'tokens', label: 'Token', render: (r) => fmtTokensShort((usageMap.get(r.id)?.input_tokens ?? 0) + (usageMap.get(r.id)?.output_tokens ?? 0)) },
    { key: 'cost', label: '费用', render: (r) => fmtUsd(usageMap.get(r.id)?.calculated_cost_micro_usd) },
    { key: 'traffic', label: '网络流量', render: (r) => fmtBytes(usageMap.get(r.id)?.estimated_traffic_bytes) },
    { key: 'last_seen_at', label: '最后上报', sortable: true, render: (r) => <span title={fmtDateTime(r.last_seen_at)}>{fmtRelative(r.last_seen_at)}</span> },
  ]

  return (
    <>
      <PageHeader title="节点" subtitle="监控节点与采集器状态" />
      <FilterBar searchPlaceholder="搜索节点名称…" onSearch={setSearch} />
      <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-4">
        <DataTable columns={columns} data={filtered} pageSize={12} onRowClick={(r) => navigate(`/nodes/${encodeURIComponent(r.id)}`)} />
      </div>
    </>
  )
}
