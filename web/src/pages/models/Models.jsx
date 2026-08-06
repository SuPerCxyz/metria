// 模型列表：模型名称/供应商/请求数/Token/费用/缓存命中率/平均响应时间/错误率。

import React, { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DataTable from '../../components/tables/DataTable'
import FilterBar from '../../components/filters/FilterBar'
import { ErrorState, LoadingSkeleton, DataQualityNote } from '../../components/feedback/Feedback'
import { api, q, rangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { fmtTokensShort, fmtUsd, fmtPct100 } from '../../services/format'

export default function Models() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const params = rangeParams(range)
  const [search, setSearch] = useState('')

  const query = useQuery(`models${q(params)}`, () => api(`/models${q(params)}`))

  const filtered = useMemo(() => {
    const list = query.data?.models || []
    if (!search) return list
    const s = search.toLowerCase()
    return list.filter((x) => (x.model || '').toLowerCase().includes(s) || (x.provider || '').toLowerCase().includes(s))
  }, [query.data, search])

  if (query.error) return <ErrorState error={query.error} onRetry={query.refresh} />
  if (query.loading) return <LoadingSkeleton rows={6} />

  const columns = [
    { key: 'model', label: '模型名称', sortable: true, render: (r) => r.model },
    { key: 'provider', label: '供应商', render: (r) => r.provider || '—' },
    { key: 'model_calls', label: '请求数', sortable: true, render: (r) => String(r.model_calls ?? 0) },
    { key: 'input_tokens', label: 'Token', sortable: true, render: (r) => fmtTokensShort((r.input_tokens ?? 0) + (r.output_tokens ?? 0)) },
    { key: 'cost', label: '费用', render: (r) => r.pricing_source === 'builtin_catalog' && !r.calculated_cost_micro_usd ? '价格未配置' : fmtUsd(r.calculated_cost_micro_usd ?? r.estimated_cost_micro_usd) },
    { key: 'clients', label: '缓存命中率', render: (r) => fmtPct100(r.cache_hit_rate) },
    { key: 'bpo', label: '平均响应时间', render: (r) => r.avg_duration_ms ? `${r.avg_duration_ms}ms` : '—' },
    { key: 'errors', label: '错误率', render: (r) => '—' },
  ]

  return (
    <>
      <PageHeader title="模型" subtitle="模型用量、费用与缓存情况" />
      <DataQualityNote kind="partial" text="价格未配置的模型将显示“价格未配置”，费用可能不完整。" />
      <FilterBar searchPlaceholder="搜索模型或供应商…" onSearch={setSearch} />
      <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-4">
        <DataTable columns={columns} data={filtered} pageSize={12} onRowClick={(r) => navigate(`/models/${encodeURIComponent(r.model)}`)} />
      </div>
    </>
  )
}
