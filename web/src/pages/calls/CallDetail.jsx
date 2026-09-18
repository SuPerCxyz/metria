// 单次模型调用详情：运行时观测字段 + 缺失说明。

import React from 'react'
import { Link, useLocation, useParams } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DetailSummary from '../../components/common/DetailSummary'
import StatusBadge from '../../components/common/StatusBadge'
import { DataQualityNote, EmptyState, ErrorState, LoadingSkeleton } from '../../components/feedback/Feedback'
import { api } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { fmtTokensShort, fmtUsd, fmtDateTime, fmtDuration, outputTokens } from '../../services/format'

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
  if (c.observability_source == null) missing.push('暂无实时观测数据；当前调用可能来自普通日志采集')
  const observedFields = [
    ['首字节', c.first_byte_latency_ms != null ? fmtDuration(c.first_byte_latency_ms) : null],
    ['首 Token', c.ttft_ms != null ? fmtDuration(c.ttft_ms) : null],
    ['生成耗时', c.generation_duration_ms != null ? fmtDuration(c.generation_duration_ms) : null],
    ['输出速度', c.output_tokens_per_second_milli != null ? `${(c.output_tokens_per_second_milli / 1000).toFixed(1)} Token/s` : null],
    ['Token 间延迟（平均）', c.inter_token_latency_avg_ms != null ? fmtDuration(c.inter_token_latency_avg_ms) : null],
    ['Token 间延迟（P95）', c.inter_token_latency_p95_ms != null ? fmtDuration(c.inter_token_latency_p95_ms) : null],
    ['停顿次数', c.stall_count != null ? String(c.stall_count) : null],
    ['停顿时长', c.stall_duration_ms != null ? fmtDuration(c.stall_duration_ms) : null],
    ['请求 payload', c.observed_request_payload_bytes != null ? `${c.observed_request_payload_bytes.toLocaleString()} B` : null],
    ['响应 payload', c.observed_response_payload_bytes != null ? `${c.observed_response_payload_bytes.toLocaleString()} B` : null],
    ['请求 Wire', c.observed_request_wire_bytes != null ? `${c.observed_request_wire_bytes.toLocaleString()} B` : null],
    ['响应 Wire', c.observed_response_wire_bytes != null ? `${c.observed_response_wire_bytes.toLocaleString()} B` : null],
    ['观测来源', c.observability_source || null],
    ['观测质量', c.observability_quality || null],
    ['Endpoint', c.endpoint || null],
    ['状态码', c.status_code != null ? String(c.status_code) : null],
    ['Finish reason', c.finish_reason || null],
    ['错误类型', c.error_kind || null],
    ['限流', c.rate_limited != null ? (c.rate_limited ? '是' : '否') : null],
    ['流式响应', c.streaming != null ? (c.streaming ? '是' : '否') : null],
    ['流式完成', c.stream_completed != null ? (c.stream_completed ? '是' : '否') : null],
    ['重试次数', c.retry_count != null ? String(c.retry_count) : null],
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
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">实时观测</h2>
        {observedFields.length > 0 ? <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
          {observedFields.map(([label, value]) => (
            <div key={label} className="bg-gray-50 dark:bg-gray-700/30 rounded-xl p-3">
              <div className="text-xs text-gray-400 dark:text-gray-500">{label}</div>
              <div className="mt-0.5 text-sm font-medium text-gray-700 dark:text-gray-200">{value}</div>
            </div>
          ))}
        </div> : <EmptyState title="暂无实时观测字段" desc="该调用没有通过 observe 产生可证明的运行时样本。" />}
        <div className="mt-4"><DataQualityNote kind="partial" text="仅展示运行时观测到的调用指标；普通日志调用保持不可用，不使用推算字节补值。" /></div>
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
