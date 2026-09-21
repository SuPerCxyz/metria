// 调用列表：当前范围内的模型调用，支持排序、加载更多和进入调用详情。

import React, { useEffect, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DataTable from '../../components/tables/DataTable'
import FilterBar from '../../components/filters/FilterBar'
import StatusBadge from '../../components/common/StatusBadge'
import { ErrorState, LoadingSkeleton } from '../../components/feedback/Feedback'
import { api, q, usageRangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { useNodeFilter } from '../../hooks/useNodeFilter'
import { fmtDateTime, fmtTokensShort, fmtUsd, outputTokens } from '../../services/format'

const PAGE_SIZE = 100

export default function Calls() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const { nodeId, clientId, model } = useNodeFilter()
  const params = usageRangeParams(range, { nodeId, clientId, model })
  const rangeKey = q(params)
  const [search, setSearch] = useState('')
  const [cursor, setCursor] = useState(null)
  const [pages, setPages] = useState({})
  const query = useQuery(`calls-list${rangeKey}&limit=${PAGE_SIZE}${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ''}`, () => api(`/calls${q({ ...params, limit: PAGE_SIZE, cursor })}`))

  useEffect(() => {
    setCursor(null)
    setPages({})
  }, [rangeKey])

  useEffect(() => {
    if (!query.data) return
    setPages((current) => ({ ...current, [cursor || 'first']: query.data }))
  }, [cursor, query.data])

  const calls = useMemo(() => Object.values(pages).flatMap((page) => page.calls || []), [pages])
  const filtered = useMemo(() => {
    const needle = search.trim().toLocaleLowerCase()
    if (!needle) return calls
    return calls.filter((call) => [call.client_id, call.model, call.provider, call.status].some((value) => String(value || '').toLocaleLowerCase().includes(needle)))
  }, [calls, search])

  if (query.error && calls.length === 0) return <ErrorState error={query.error} onRetry={query.refresh} />
  if (query.loading && calls.length === 0) return <LoadingSkeleton rows={8} />

  const columns = [
    { key: 'started_at', label: '调用时间', sortValue: (r) => r.started_at, render: (r) => fmtDateTime(r.started_at) },
    { key: 'client_id', label: 'Agent', render: (r) => r.client_id || '—' },
    { key: 'model', label: '模型', render: (r) => <span title={r.model || ''} className="block max-w-[16rem] truncate">{r.model || '—'}</span> },
    { key: 'provider', label: '供应商', hideWhenEmpty: true, render: (r) => r.provider || '—' },
    { key: 'status', label: '状态', render: (r) => <StatusBadge status={r.status} /> },
    { key: 'input_tokens', label: '输入 Token', sortValue: (r) => r.input_tokens, render: (r) => fmtTokensShort(r.input_tokens) },
    { key: 'output_tokens', label: '输出 Token', sortValue: (r) => outputTokens(r), render: (r) => fmtTokensShort(outputTokens(r)) },
    { key: 'cost', label: '费用', sortValue: (r) => r.reported_cost_micro_usd ?? r.calculated_cost_micro_usd ?? r.estimated_cost_micro_usd, render: (r) => fmtUsd(r.reported_cost_micro_usd ?? r.calculated_cost_micro_usd ?? r.estimated_cost_micro_usd) },
  ]

  const nextCursor = query.data?.next_cursor

  return (
    <>
      <PageHeader title="调用" subtitle="查看当前时间范围内的模型调用与 Token、费用" />
      <FilterBar searchPlaceholder="搜索 Agent、模型或状态…" onSearch={setSearch} />
      <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-4">
        <DataTable columns={columns} data={filtered} pageSize={20} onRowClick={(r) => navigate(`/calls/${encodeURIComponent(r.id)}`)} />
        {query.error && <p className="mt-3 text-sm text-amber-600 dark:text-amber-400">下一页加载失败：{query.error.message}</p>}
        {nextCursor && !search && (
          <div className="mt-4 flex justify-center">
            <button type="button" disabled={query.loading} onClick={() => setCursor(nextCursor)} className="btn-sm btn-secondary">
              {query.loading ? '加载中…' : '加载更多调用'}
            </button>
          </div>
        )}
      </div>
    </>
  )
}
