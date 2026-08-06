import { useMemo, useRef, useState } from 'preact/hooks'
import { api, q } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { getRange, useQuery } from '../hooks/useQuery'
import { fmtBytes, fmtDateTime, fmtTokens } from '../lib/format'
import { t } from '../lib/i18n'
import { nav } from '../lib/router'

type SortKey = 'started_at' | 'title' | 'client_id' | 'model' | 'message_count' | 'model_call_count' | 'input_tokens' | 'estimated_total_bytes'

const SORTABLE: { key: SortKey; label: string }[] = [
  { key: 'started_at', label: '开始' },
  { key: 'title', label: '标题' },
  { key: 'client_id', label: 'Agent 工具' },
  { key: 'model', label: '模型' },
  { key: 'message_count', label: '消息' },
  { key: 'model_call_count', label: 'Calls' },
  { key: 'input_tokens', label: 'Input' },
  { key: 'estimated_total_bytes', label: '估算流量' },
]

const ROW_H = 38
const VIEW_H = 520

/** 窗口化渲染：只渲染可视区附近的行，实现虚拟滚动。 */
function VirtualRows({ rows }: { rows: any[] }) {
  const ref = useRef<HTMLDivElement>(null)
  const [scrollTop, setScrollTop] = useState(0)
  const start = Math.max(0, Math.floor(scrollTop / ROW_H) - 8)
  const end = Math.min(rows.length, Math.ceil((scrollTop + VIEW_H) / ROW_H) + 8)
  const visible = rows.slice(start, end)

  return (
    <div
      ref={ref}
      style={{ height: VIEW_H, overflowY: 'auto', position: 'relative' }}
      onScroll={(e) => setScrollTop((e.target as HTMLDivElement).scrollTop)}
    >
      <div style={{ height: rows.length * ROW_H, position: 'relative' }}>
        {visible.map((s: any, i: number) => {
          const rowIdx = start + i
          return (
            <div
              key={s.id}
              class="vrow clickable"
              style={{ position: 'absolute', top: rowIdx * ROW_H, left: 0, right: 0, height: ROW_H }}
              onClick={() => nav(`sessions/${s.id}`)}
            >
              <span class="vcell title">{s.title || s.source_session_id}</span>
              <span class="vcell">{s.client_id}</span>
              <span class="vcell">{s.model || '—'}</span>
              <span class="vcell">{fmtDateTime(s.started_at)}</span>
              <span class="vcell num">{s.message_count}</span>
              <span class="vcell num">{s.model_call_count}</span>
              <span class="vcell num">{fmtTokens(s.input_tokens)}</span>
              <span class="vcell num">{fmtTokens(s.output_tokens)}</span>
              <span class="vcell num">{fmtBytes(s.estimated_total_bytes)}</span>
            </div>
          )
        })}
      </div>
    </div>
  )
}

export function Sessions() {
  const range = getRange()
  const params = { from: range.from, to: range.to, timezone: range.timezone }
  const [sortKey, setSortKey] = useState<SortKey>('started_at')
  const [sortDir, setSortDir] = useState<1 | -1>(-1)
  const sessions = useQuery<any>(`sessions${q(params)}`, () => api(`/sessions${q(params)}`))
  if (sessions.error) return <ErrorBox error={sessions.error} onRetry={sessions.refresh} />
  if (sessions.loading) return <Empty text={t('common.loading')} />

  const rows = useMemo(() => {
    const list = sessions.data?.sessions || []
    return [...list].sort((a, b) => {
      const av = a[sortKey] ?? ''
      const bv = b[sortKey] ?? ''
      const cmp = typeof av === 'number' ? av - (bv as number) : String(av).localeCompare(String(bv))
      return cmp * sortDir
    })
  }, [sessions.data, sortKey, sortDir])

  const toggleSort = (key: SortKey) => {
    if (key === sortKey) setSortDir((d) => (d === 1 ? -1 : 1))
    else {
      setSortKey(key)
      setSortDir(key === 'started_at' ? -1 : 1)
    }
  }

  return (
    <div class="page">
      <h2>{t('sessions.title')}</h2>
      <Card>
        <div class="vhead">
          {SORTABLE.map((col) => (
            <span
              key={col.key}
              class={`vcell clickable${col.key === sortKey ? ' sorted' : ''}`}
              onClick={() => toggleSort(col.key)}
            >
              {col.label}
              {col.key === sortKey ? (sortDir === 1 ? ' ▲' : ' ▼') : ''}
            </span>
          ))}
        </div>
        {rows.length === 0 ? <Empty /> : <VirtualRows rows={rows} />}
      </Card>
    </div>
  )
}

export function SessionDetail({ id }: { id: string }) {
  const session = useQuery<any>(`session${id}`, () => api(`/sessions/${encodeURIComponent(id)}`))
  const calls = useQuery<any>(`session-calls${id}`, () => api(`/sessions/${encodeURIComponent(id)}/calls`))
  const tools = useQuery<any>(`session-tools${id}`, () => api(`/sessions/${encodeURIComponent(id)}/tools`))
  const subagents = useQuery<any>(`session-sa${id}`, () => api(`/sessions/${encodeURIComponent(id)}/subagents`))
  const timeline = useQuery<any>(`session-tl${id}`, () => api(`/sessions/${encodeURIComponent(id)}/timeline`))

  if (session.error) return <ErrorBox error={session.error} onRetry={session.refresh} />
  if (session.loading) return <Empty text={t('common.loading')} />
  const s = session.data?.session || {}

  const callList: any[] = calls.data?.calls || []
  const maxTraffic = Math.max(0, ...callList.map((c: any) => Number(c.estimated_total_bytes) || 0))

  const saChildren = new Map(
    (subagents.data?.children || []).map((c: any) => [c.id, c]),
  )
  const saBySource = new Map<string, any>()
  for (const c of subagents.data?.children || []) {
    if (c.source_session_id) saBySource.set(c.source_session_id, c)
  }
  const saRelations: any[] = subagents.data?.relations || []

  return (
    <div class="page">
      <button type="button" class="btn" onClick={() => nav('sessions')}>
        ← {t('sessions.title')}
      </button>
      <h2>{s.title || s.source_session_id}</h2>
      <div class="stat-grid">
        {[
          [t('common.node'), s.node_id],
          [t('sessions.client'), s.client_id],
          [t('common.model'), s.model || t('common.notAvailable')],
          [t('common.provider'), s.provider || t('common.notAvailable')],
          [t('common.startTime'), fmtDateTime(s.started_at)],
          [t('sessions.messages'), String(s.message_count ?? 0)],
          [t('sessions.tools'), String(s.tool_call_count ?? 0)],
          [t('sessions.subagents'), String(s.subagent_count ?? 0)],
          [t('sessions.modelCalls'), String(s.model_call_count ?? 0)],
          [t('common.input'), fmtTokens(s.input_tokens)],
          [t('common.output'), fmtTokens(s.output_tokens)],
          [t('common.cacheRead'), fmtTokens(s.cache_read_tokens)],
          [t('sessions.traffic'), fmtBytes(s.estimated_total_bytes)],
          [t('common.cost'), `$${((s.reported_cost_micro_usd ?? s.calculated_cost_micro_usd ?? 0) / 1e6).toFixed(4)}`],
        ].map(([label, value]) => (
          <div class="kv">
            <span class="kv-label">{label}</span>
            <span class="kv-value">{value}</span>
          </div>
        ))}
      </div>

      <Card title={t('sessions.waterfall')}>
        <div class="waterfall">
          {callList.length === 0 && <div class="sa-empty">{t('common.empty')}</div>}
          {callList.map((c: any, i: number) => {
            const bytes = Number(c.estimated_total_bytes) || 0
            const pctW = maxTraffic > 0 ? Math.max(1, (bytes / maxTraffic) * 100) : 0
            const prev = callList[i - 1]
            const switched = prev && prev.model && c.model && prev.model !== c.model
            return (
              <div key={c.id} class="wf-row">
                <span class="wf-time">{fmtDateTime(c.started_at)}</span>
                <span class="wf-bar-wrap">
                  <span class="wf-bar" style={{ width: `${pctW}%` }} />
                </span>
                <span class="wf-model" onClick={() => nav(`calls/${c.id}`)} title={c.id}>
                  {c.model || t('common.notAvailable')}
                  {switched && <span class="wf-switch"> ⚠ {t('sessions.switchBadge')}</span>}
                </span>
                <span class="wf-val">{fmtBytes(bytes)}</span>
              </div>
            )
          })}
        </div>
      </Card>

      <Card title={t('sessions.subagentTree')}>
        {saRelations.length === 0 && <div class="sa-empty">{t('sessions.noSubagents')}</div>}
        {saRelations.map((r: any) => {
          const child = saChildren.get(r.child_session_id) || saBySource.get(r.child_session_id)
          return (
            <div key={r.id} class="sa-node">
              <span class="sa-relation">{r.relation}</span>
              {child ? (
                <span
                  class="sa-child"
                  onClick={() => nav(`sessions/${child.id}`)}
                  title={child.source_session_id}
                >
                  {child.title || child.source_session_id}
                </span>
              ) : (
                <span title={r.child_session_id}>{r.child_session_id}</span>
              )}
              {child && (
                <span class="tl-meta">
                  {t('sessions.childSession')}: {child.message_count} msg · {fmtBytes(child.estimated_total_bytes)}
                </span>
              )}
            </div>
          )
        })}
      </Card>

      <div class="grid-2">
        <Card title={t('sessions.toolCall')}>
          <table class="table">
            <thead>
              <tr>
                <th>{t('sessions.tool')}</th>
                <th>{t('common.status')}</th>
                <th>{t('sessions.inputBytes')}</th>
                <th>{t('sessions.outputBytes')}</th>
              </tr>
            </thead>
            <tbody>
              {(tools.data?.tools || []).map((tool: any) => (
                <tr key={tool.id}>
                  <td>{tool.name}</td>
                  <td>{tool.status}</td>
                  <td>{fmtBytes(tool.input_length)}</td>
                  <td>{fmtBytes(tool.output_length)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>

        <Card title={t('sessions.timeline')}>
          <div class="timeline">
            {(timeline.data?.messages || []).map((m: any) => (
              <div key={m.id} class={`tl-item role-${m.role}`}>
                <span class="tl-role">{m.role}</span>
                <span class="tl-type">{m.content_type}</span>
                <span class="tl-content">
                  {m.redacted ? t('sessions.redacted') : String(m.content || '').slice(0, 200)}
                </span>
                <span class="tl-meta">
                  {m.utf8_bytes} B · {fmtDateTime(m.created_at)}
                </span>
              </div>
            ))}
          </div>
        </Card>
      </div>
    </div>
  )
}
