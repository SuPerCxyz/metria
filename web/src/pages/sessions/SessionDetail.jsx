// 会话详情：摘要 + 趋势图 + Token 构成 + 模型调用列表 + 错误异常。

import React, { useMemo, useState } from 'react'
import { Link, useParams, useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DetailSummary from '../../components/common/DetailSummary'
import StatusBadge from '../../components/common/StatusBadge'
import DataTable from '../../components/tables/DataTable'
import TrendChart from '../../components/charts/TrendChart'
import Segmented from '../../components/ui/Segmented'
import { ErrorState, LoadingSkeleton, EmptyState } from '../../components/feedback/Feedback'
import { api } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useNodeNames } from '../../hooks/useNodeNames'
import { fmtTokensShort, fmtUsd, fmtBytes, fmtDateTime, fmtDuration, fmtSessionTitle, sumTokens } from '../../services/format'

const TREND_TABS = [
  { key: 'tokens', label: 'Token' },
  { key: 'cost', label: '费用' },
  { key: 'traffic', label: '流量' },
  { key: 'latency', label: '延迟' },
]

export default function SessionDetail() {
  const { id } = useParams()
  const navigate = useNavigate()
  const [trendTab, setTrendTab] = useState('tokens')

  const query = useQuery(`session-detail-${id}`, () => api(`/sessions/${encodeURIComponent(id)}`))
  const calls = useQuery(`session-calls-${id}`, () => api(`/sessions/${encodeURIComponent(id)}/calls`))
  const timeline = useQuery(`session-timeline-${id}`, () => api(`/sessions/${encodeURIComponent(id)}/timeline`))
  const nodeNames = useNodeNames()

  const callList = calls.data?.calls || []
  const messageList = timeline.data?.messages || []

  const trendData = useMemo(() => {
    const pts = callList
    switch (trendTab) {
      case 'tokens': return { labels: pts.map((p) => p.started_at), values: pts.map((p) => sumTokens(p)) }
      case 'cost': return { labels: pts.map((p) => p.started_at), values: pts.map((p) => p.reported_cost_micro_usd ?? p.calculated_cost_micro_usd ?? p.estimated_cost_micro_usd ?? 0) }
      case 'traffic': return { labels: pts.map((p) => p.started_at), values: pts.map((p) => p.estimated_total_bytes ?? 0) }
      case 'latency': return { labels: pts.map((p) => p.started_at), values: pts.map((p) => p.duration_ms ?? 0) }
      default: return { labels: [], values: [] }
    }
  }, [callList, trendTab])

  const formatY = (v) => {
    if (trendTab === 'tokens') return fmtTokensShort(v)
    if (trendTab === 'cost') return fmtUsd(v)
    if (trendTab === 'traffic') return fmtBytes(v)
    return `${v}ms`
  }

  const callColumns = [
    { key: 'started_at', label: '调用时间', sortable: true, render: (r) => fmtDateTime(r.started_at) },
    { key: 'model', label: '模型', render: (r) => r.model || '—' },
    { key: 'input_tokens', label: '输入 Token', render: (r) => fmtTokensShort(r.input_tokens) },
    { key: 'output_tokens', label: '输出 Token', render: (r) => fmtTokensShort(r.output_tokens) },
    { key: 'cache_read_tokens', label: '缓存 Token', render: (r) => fmtTokensShort(r.cache_read_tokens) },
    { key: 'calculated_cost_micro_usd', label: '费用', render: (r) => fmtUsd(r.reported_cost_micro_usd ?? r.calculated_cost_micro_usd ?? r.estimated_cost_micro_usd) },
    { key: 'duration_ms', label: '响应时间', render: (r) => fmtDuration(r.duration_ms) },
    { key: 'status', label: '状态', render: (r) => <StatusBadge status={r.status} /> },
  ]

  const errors = (callList || []).filter((c) => c.status && !['success', 'ok', 'completed'].includes(String(c.status).toLowerCase()))

  if (query.error) return <ErrorState error={query.error} onRetry={query.refresh} />
  if (query.loading) return <LoadingSkeleton rows={8} />
  const s = query.data?.session || {}

  return (
    <>
      <PageHeader
        back={<Link to="/sessions" className="text-sm text-indigo-600 dark:text-indigo-400 hover:underline">← 返回会话</Link>}
        title="会话详情"
        subtitle={<span>{s.client_id} · {nodeNames[s.node_id] || s.node_id} · <StatusBadge status={s.status} /></span>}
      />

      <DetailSummary
        items={[
          { label: '标题', value: <span className="block max-w-[14rem] truncate" title={fmtSessionTitle(s.title, s.started_at)}>{fmtSessionTitle(s.title, s.started_at)}</span> },
          { label: '会话 ID', value: <span className="block min-w-0 truncate font-sans" title={s.id || ''} aria-label={`完整会话 ID：${s.id || '未知'}`}>{compactSessionId(s.id)}</span> },
          { label: 'Agent', value: s.client_id || '—' },
          { label: '节点', value: nodeNames[s.node_id] || s.node_id || '—' },
          { label: '开始时间', value: fmtDateTime(s.started_at) },
          { label: '结束时间', value: fmtDateTime(s.last_activity_at || s.ended_at || s.started_at) },
          { label: '持续时间', value: fmtDuration((new Date(s.last_activity_at || s.ended_at || Date.now()) - new Date(s.started_at)).valueOf()) },
          { label: '调用次数', value: String(s.model_call_count ?? 0) },
          { label: '总 Token', value: fmtTokensShort(sumTokens(s)) },
          { label: '总费用', value: fmtUsd(s.reported_cost_micro_usd ?? s.calculated_cost_micro_usd ?? s.estimated_cost_micro_usd) },
          { label: '总流量', value: fmtBytes(s.estimated_total_bytes) },
          { label: '模型', value: s.model || '—' },
        ]}
      />

      <StartupContext session={s} nodeName={nodeNames[s.node_id] || s.node_id} />

      <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <div className="flex items-center justify-between mb-4 flex-wrap gap-3">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">会话趋势</h2>
          <Segmented items={TREND_TABS} value={trendTab} onChange={setTrendTab} />
        </div>
        {trendData.labels.length === 0 ? <EmptyState title="该会话暂无模型调用" /> : <TrendChart labels={trendData.labels} values={trendData.values} height={320} formatY={formatY} />}
      </div>

      <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">Token 构成</h2>
        <div className="flex flex-wrap gap-6">
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-bold text-gray-800 dark:text-gray-100 tabular-nums">{fmtTokensShort(s.input_tokens)}</span>
            <span className="text-sm text-gray-500 dark:text-gray-400">输入</span>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-bold text-gray-800 dark:text-gray-100 tabular-nums">{fmtTokensShort(s.output_tokens)}</span>
            <span className="text-sm text-gray-500 dark:text-gray-400">输出</span>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-bold text-gray-800 dark:text-gray-100 tabular-nums">{fmtTokensShort(s.cache_read_tokens)}</span>
            <span className="text-sm text-gray-500 dark:text-gray-400">缓存</span>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-bold text-gray-800 dark:text-gray-100 tabular-nums">{fmtTokensShort(s.reasoning_tokens)}</span>
            <span className="text-sm text-gray-500 dark:text-gray-400">推理</span>
          </div>
        </div>
      </div>

      <ConversationTimeline query={timeline} messages={messageList} />

      <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-4">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 p-2">模型调用列表</h2>
        <DataTable columns={callColumns} data={callList} pageSize={12} onRowClick={(r) => navigate(`/calls/${encodeURIComponent(r.id)}`, { state: { from: `/sessions/${encodeURIComponent(id)}` } })} />
      </div>

      {errors.length > 0 && (
        <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-red-200 dark:border-red-800/60 p-6">
          <h2 className="text-lg font-bold text-red-600 dark:text-red-400 mb-4">错误和异常</h2>
          <div className="space-y-2">
            {errors.slice(0, 10).map((e) => (
              <div key={e.id} className="flex items-center justify-between px-3 py-2 rounded-lg bg-red-50 dark:bg-red-400/5 text-sm">
                <span className="text-gray-700 dark:text-gray-200">{e.model || '—'} · {fmtDateTime(e.started_at)}</span>
                <StatusBadge status={e.status} />
              </div>
            ))}
          </div>
        </div>
      )}
    </>
  )
}

function compactSessionId(value) {
  if (!value) return '—'
  if (value.length <= 28) return value
  return `${value.slice(0, 18)}…${value.slice(-7)}`
}

function StartupContext({ session, nodeName }) {
  return (
    <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
      <div className="mb-4">
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">会话启动信息</h2>
        <p className="mt-1 text-xs text-gray-400 dark:text-gray-500">只读展示已采集的会话上下文，不执行或恢复历史命令。</p>
      </div>
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <ContextItem label="Agent" value={session.client_id || '—'} />
        <ContextItem label="模型" value={session.model || '—'} />
        <ContextItem label="节点" value={nodeName || '—'} />
        <ContextItem label="开始时间" value={fmtDateTime(session.started_at)} />
      </div>
      <div className="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2">
        <ContextItem label="来源会话 ID" value={session.source_session_id || '—'} />
        <div className="rounded-xl bg-gray-50 p-3 dark:bg-gray-700/30">
          <div className="text-xs text-gray-400 dark:text-gray-500">启动命令</div>
          {session.startup_command ? (
            <pre className="mt-1 max-h-32 overflow-auto whitespace-pre-wrap break-words text-xs text-gray-700 dark:text-gray-200">{session.startup_command}</pre>
          ) : (
            <div className="mt-1 text-sm text-gray-500 dark:text-gray-400">未记录启动命令</div>
          )}
        </div>
      </div>
    </div>
  )
}

function ContextItem({ label, value }) {
  return (
    <div className="min-w-0 rounded-xl bg-gray-50 p-3 dark:bg-gray-700/30">
      <div className="text-xs text-gray-400 dark:text-gray-500">{label}</div>
      <div className="mt-1 truncate text-sm font-semibold text-gray-700 dark:text-gray-200" title={value}>{value}</div>
    </div>
  )
}

function ConversationTimeline({ query, messages }) {
  return (
    <div className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
      <div className="flex items-center justify-between gap-3 mb-4">
        <div>
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">对话列表</h2>
          <p className="mt-1 text-xs text-gray-400 dark:text-gray-500">按会话发生顺序展示已采集的消息。</p>
        </div>
        {messages.length > 0 && <span className="text-xs text-gray-400 dark:text-gray-500">{messages.length} 条</span>}
      </div>
      {query.loading && <LoadingSkeleton rows={4} />}
      {query.error && <ErrorState error={query.error} onRetry={query.refresh} />}
      {!query.loading && !query.error && messages.length === 0 && <EmptyState title="暂无对话内容" desc="当前会话没有可展示的消息，或内容保存策略未记录正文。" />}
      {!query.loading && !query.error && messages.length > 0 && (
        <div className="space-y-3">
          {messages.map((message, index) => <ConversationMessage key={message.id || index} message={message} />)}
        </div>
      )}
    </div>
  )
}

function ConversationMessage({ message }) {
  const role = {
    user: '用户',
    assistant: '助手',
    system: '系统',
    tool: '工具',
  }[message.role] || message.role || '消息'
  const roleTone = message.role === 'user'
    ? 'border-indigo-200 dark:border-indigo-800/60'
    : message.role === 'assistant'
      ? 'border-emerald-200 dark:border-emerald-800/60'
      : 'border-gray-200 dark:border-gray-700/60'
  const content = message.redacted
    ? '正文已按隐私策略脱敏。'
    : message.content || (message.content_length > 0
      ? `正文未保存（仅保留元数据，长度 ${message.content_length} 字符）。`
      : '无正文内容。')
  return (
    <article className={`rounded-xl border-l-4 bg-gray-50 p-4 dark:bg-gray-700/30 ${roleTone}`}>
      <div className="mb-2 flex items-center justify-between gap-3">
        <span className="text-sm font-semibold text-gray-700 dark:text-gray-200">{role}</span>
        <time className="shrink-0 text-xs text-gray-400 dark:text-gray-500 tabular-nums">{fmtDateTime(message.created_at)}</time>
      </div>
      <div className={`max-h-80 overflow-auto whitespace-pre-wrap break-words text-sm leading-6 ${message.content ? 'text-gray-700 dark:text-gray-200' : 'text-gray-400 dark:text-gray-500'}`}>
        {content}
      </div>
    </article>
  )
}
