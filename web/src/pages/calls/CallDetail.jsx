// 单次模型调用详情：日志推导性能字段 + 缺失说明。

import React from 'react'
import { Link, useLocation, useParams } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DetailSummary from '../../components/common/DetailSummary'
import StatusBadge from '../../components/common/StatusBadge'
import { DataQualityNote, EmptyState, ErrorState, LoadingSkeleton } from '../../components/feedback/Feedback'
import { api } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { fmtTokensShort, fmtUsd, fmtDateTime, fmtDuration, outputTokens } from '../../services/format'

/// 两个 RFC3339 时间之间的毫秒差；缺失或倒序返回 null。
function msBetween(from, to) {
  if (!from || !to) return null
  const start = Date.parse(from)
  const end = Date.parse(to)
  if (Number.isNaN(start) || Number.isNaN(end) || end < start) return null
  return end - start
}

export default function CallDetail() {
  const { id } = useParams()
  const location = useLocation()
  const query = useQuery(`call-detail-${id}`, () => api(`/calls/${encodeURIComponent(id)}`))

  if (query.error) return <ErrorState error={query.error} onRetry={query.refresh} />
  if (query.loading) return <LoadingSkeleton rows={6} />
  const c = query.data?.call || {}

  const missing = []
  if (c.input_tokens == null) missing.push('usage 缺失：客户端日志未记录本次调用 Token')
  if (c.calculated_cost_micro_usd == null && c.reported_cost_micro_usd == null) missing.push('费用缺失：无 reported/calculated 成本')

  const firstOutput = c.first_response_at || c.first_token_at || c.first_byte_at
  const ttft = msBetween(c.started_at, firstOutput)

  const performanceFields = [
    ['首个可观察输出', ttft != null ? fmtDuration(ttft) : null],
    ['状态码', c.status_code != null ? String(c.status_code) : null],
  ].filter(([, value]) => value != null)

  return (
    <>
      <PageHeader
        back={<Link to={location.state?.from || '/sessions'} className="text-sm text-indigo-600 dark:text-indigo-400 hover:underline">← 返回</Link>}
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
          { label: '输出 Token', value: fmtTokensShort(outputTokens(c)) },
          { label: '缓存 Token', value: fmtTokensShort(c.cache_read_tokens) },
          { label: '费用', value: fmtUsd(c.reported_cost_micro_usd ?? c.calculated_cost_micro_usd ?? c.estimated_cost_micro_usd) },
        ]}
      />

      <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">性能</h2>
        {performanceFields.length > 0 ? <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
          {performanceFields.map(([label, value]) => (
            <div key={label} className="bg-gray-50 dark:bg-gray-700/30 rounded-xl p-3">
              <div className="text-xs text-gray-400 dark:text-gray-500">{label}</div>
              <div className="mt-0.5 text-sm font-medium text-gray-700 dark:text-gray-200">{value}</div>
            </div>
          ))}
        </div> : <EmptyState title="暂无可推导的性能字段" desc="客户端日志没有记录本次调用的可证明时间事件。" />}
        <div className="mt-4"><DataQualityNote kind="partial" text="性能值由客户端日志时间戳推导，缺失时保持不可用，不使用推算值补齐。" /></div>
      </div>

      {missing.length > 0 && (
        <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-amber-200 dark:border-amber-800/60 p-6">
          <h2 className="text-lg font-bold text-amber-600 dark:text-amber-400 mb-2">缺失说明</h2>
          <ul className="space-y-1">
            {missing.map((m, i) => <li key={i} className="text-sm text-gray-600 dark:text-gray-300">· {m}</li>)}
          </ul>
        </div>
      )}
    </>
  )
}
