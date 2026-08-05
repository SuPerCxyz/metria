import { api, q } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { getRange, useQuery } from '../hooks/useQuery'
import { fmtBytes, fmtDateTime, fmtTokens } from '../lib/format'
import { nav } from '../lib/router'

export function Sessions() {
  const range = getRange()
  const params = { from: range.from, to: range.to, timezone: range.timezone }
  const sessions = useQuery<any>(`sessions${q(params)}`, () => api(`/sessions${q(params)}`))
  if (sessions.error) return <ErrorBox error={sessions.error} onRetry={sessions.refresh} />
  if (sessions.loading) return <Empty text="加载中…" />

  return (
    <div class="page">
      <h2>会话</h2>
      <Card>
        <table class="table">
          <thead>
            <tr>
              <th>标题</th>
              <th>Agent 工具</th>
              <th>模型</th>
              <th>开始</th>
              <th>消息</th>
              <th>Calls</th>
              <th>Input</th>
              <th>Output</th>
              <th>估算流量</th>
            </tr>
          </thead>
          <tbody>
            {(sessions.data?.sessions || []).map((s: any) => (
              <tr key={s.id} class="clickable" onClick={() => nav(`sessions/${s.id}`)}>
                <td>{s.title || s.source_session_id}</td>
                <td>{s.client_id}</td>
                <td>{s.model || '—'}</td>
                <td>{fmtDateTime(s.started_at)}</td>
                <td>{s.message_count}</td>
                <td>{s.model_call_count}</td>
                <td>{fmtTokens(s.input_tokens)}</td>
                <td>{fmtTokens(s.output_tokens)}</td>
                <td>{fmtBytes(s.estimated_total_bytes)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  )
}

export function SessionDetail({ id }: { id: string }) {
  const session = useQuery<any>(`session${id}`, () => api(`/sessions/${encodeURIComponent(id)}`))
  const calls = useQuery<any>(`session-calls${id}`, () => api(`/sessions/${encodeURIComponent(id)}/calls`))
  const tools = useQuery<any>(`session-tools${id}`, () => api(`/sessions/${encodeURIComponent(id)}/tools`))
  const timeline = useQuery<any>(`session-tl${id}`, () => api(`/sessions/${encodeURIComponent(id)}/timeline`))

  if (session.error) return <ErrorBox error={session.error} onRetry={session.refresh} />
  if (session.loading) return <Empty text="加载中…" />
  const s = session.data?.session || {}

  return (
    <div class="page">
      <button type="button" class="btn" onClick={() => nav('sessions')}>
        ← 会话
      </button>
      <h2>{s.title || s.source_session_id}</h2>
      <div class="stat-grid">
        {[
          ['Node', s.node_id],
          ['Agent 工具', s.client_id],
          ['模型', s.model || '—'],
          ['Provider', s.provider || '—'],
          ['开始', fmtDateTime(s.started_at)],
          ['消息', String(s.message_count ?? 0)],
          ['Tool 调用', String(s.tool_call_count ?? 0)],
          ['子 Agent', String(s.subagent_count ?? 0)],
          ['Model Calls', String(s.model_call_count ?? 0)],
          ['Input', fmtTokens(s.input_tokens)],
          ['Output', fmtTokens(s.output_tokens)],
          ['Cache Read', fmtTokens(s.cache_read_tokens)],
          ['估算流量', fmtBytes(s.estimated_total_bytes)],
          ['费用', `$${((s.reported_cost_micro_usd ?? s.calculated_cost_micro_usd ?? 0) / 1e6).toFixed(4)}`],
        ].map(([label, value]) => (
          <div class="kv">
            <span class="kv-label">{label}</span>
            <span class="kv-value">{value}</span>
          </div>
        ))}
      </div>

      <div class="grid-2">
        <Card title="每次调用估算流量">
          <table class="table">
            <thead>
              <tr>
                <th>时间</th>
                <th>模型</th>
                <th>Input</th>
                <th>Output</th>
                <th>估算流量</th>
              </tr>
            </thead>
            <tbody>
              {(calls.data?.calls || []).map((c: any) => (
                <tr key={c.id} class="clickable" onClick={() => nav(`calls/${c.id}`)}>
                  <td>{fmtDateTime(c.started_at)}</td>
                  <td>{c.model || '—'}</td>
                  <td>{fmtTokens(c.input_tokens)}</td>
                  <td>{fmtTokens(c.output_tokens)}</td>
                  <td>{fmtBytes(c.estimated_total_bytes ?? c.calculated_cost_micro_usd)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
        <Card title="Tool 调用">
          <table class="table">
            <thead>
              <tr>
                <th>工具</th>
                <th>状态</th>
                <th>输入字节</th>
                <th>输出字节</th>
              </tr>
            </thead>
            <tbody>
              {(tools.data?.tools || []).map((t: any) => (
                <tr key={t.id}>
                  <td>{t.name}</td>
                  <td>{t.status}</td>
                  <td>{fmtBytes(t.input_length)}</td>
                  <td>{fmtBytes(t.output_length)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      </div>

      <Card title="Session Timeline">
        <div class="timeline">
          {(timeline.data?.messages || []).map((m: any) => (
            <div key={m.id} class={`tl-item role-${m.role}`}>
              <span class="tl-role">{m.role}</span>
              <span class="tl-type">{m.content_type}</span>
              <span class="tl-content">{m.redacted ? '[已脱敏，仅元数据]' : String(m.content || '').slice(0, 200)}</span>
              <span class="tl-meta">
                {m.utf8_bytes} B · {fmtDateTime(m.created_at)}
              </span>
            </div>
          ))}
        </div>
      </Card>
    </div>
  )
}
