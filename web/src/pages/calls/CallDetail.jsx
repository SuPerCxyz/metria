// 单次模型调用详情：全字段 + 估算区间 + 缺失说明。

import React from 'react'
import { Link, useParams } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DetailSummary from '../../components/common/DetailSummary'
import StatusBadge from '../../components/common/StatusBadge'
import { DataQualityNote, ErrorState, LoadingSkeleton } from '../../components/feedback/Feedback'
import { api } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { fmtTokensShort, fmtUsd, fmtBytes, fmtDateTime, fmtDuration } from '../../services/format'

export default function CallDetail() {
  const { id } = useParams()
  const query = useQuery(`call-detail-${id}`, () => api(`/calls/${encodeURIComponent(id)}`))

  if (query.error) return <ErrorState error={query.error} />
  if (query.loading) return <LoadingSkeleton rows={6} />
  const c = query.data?.call || {}
  const tr = query.data?.traffic || {}

  const missing = []
  if (c.input_tokens == null) missing.push('usage 缺失：客户端日志未记录本次调用 Token')
  if (c.calculated_cost_micro_usd == null && c.reported_cost_micro_usd == null) missing.push('费用缺失：无 reported/calculated 成本')
  if (!tr.estimation_source || tr.estimation_source === 'unavailable') missing.push('流量估算不可用（unavailable），未硬造数值')

  return (
    <>
      <PageHeader
        back={<Link to="/sessions" className="text-sm text-indigo-600 dark:text-indigo-400 hover:underline">← 返回</Link>}
        title={`模型调用 ${c.id}`}
        subtitle={<StatusBadge status={c.status} />}
      />

      <DetailSummary
        items={[
          { label: '模型', value: c.model || c.model_raw || '—' },
          { label: '供应商', value: c.provider || c.provider_raw || '—' },
          { label: 'Agent', value: c.client_id || '—' },
          { label: '开始时间', value: fmtDateTime(c.started_at) },
          { label: '响应时间', value: fmtDuration(c.duration_ms) },
          { label: '输入 Token', value: fmtTokensShort(c.input_tokens) },
          { label: '输出 Token', value: fmtTokensShort(c.output_tokens) },
          { label: '缓存 Token', value: fmtTokensShort(c.cache_read_tokens) },
          { label: '费用', value: fmtUsd(c.calculated_cost_micro_usd ?? c.estimated_cost_micro_usd) },
        ]}
      />

      <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">估算流量</h2>
        <div className="flex items-baseline gap-4 flex-wrap">
          <span className="text-3xl font-bold text-gray-800 dark:text-gray-100 tabular-nums">{fmtBytes(tr.estimated_total_wire_bytes)}</span>
          <span className="text-sm text-gray-400 dark:text-gray-500">范围 {fmtBytes(tr.lower_bound_bytes)} ~ {fmtBytes(tr.upper_bound_bytes)}</span>
          {tr.confidence != null && <span className="text-sm text-gray-400 dark:text-gray-500">置信度 {(tr.confidence * 100).toFixed(0)}%</span>}
        </div>
        <div className="mt-4 grid grid-cols-2 sm:grid-cols-3 gap-4">
          {[
            ['请求流量', fmtBytes(tr.estimated_request_wire_bytes)],
            ['响应流量', fmtBytes(tr.estimated_response_wire_bytes)],
            ['估算来源', tr.estimation_source || '—'],
            ['上下文传输', tr.context_transport_mode || '—'],
            ['Cache 行为', tr.cache_transport_behavior || '—'],
            ['Traffic Profile', tr.profile_id ? `${tr.profile_id} v${tr.profile_version ?? ''}` : '未命中'],
          ].map(([label, value]) => (
            <div key={label} className="bg-gray-50 dark:bg-gray-700/30 rounded-xl p-3">
              <div className="text-xs text-gray-400 dark:text-gray-500">{label}</div>
              <div className="mt-0.5 text-sm font-medium text-gray-700 dark:text-gray-200">{value}</div>
            </div>
          ))}
        </div>
        <div className="mt-4">
          <DataQualityNote kind="estimated" text="以上为估算流量，不代表网卡真实流量或云厂商计费流量。" />
        </div>
      </div>

      {missing.length > 0 && (
        <div className="mt-6 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-amber-200 dark:border-amber-800/60 p-6">
          <h2 className="text-lg font-bold text-amber-600 dark:text-amber-400 mb-2">缺失说明</h2>
          <ul className="space-y-1">
            {missing.map((m, i) => <li key={i} className="text-sm text-gray-600 dark:text-gray-300">· {m}</li>)}
          </ul>
        </div>
      )}
    </>
  )
}
