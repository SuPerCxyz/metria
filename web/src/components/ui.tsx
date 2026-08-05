import { useEffect } from 'preact/hooks'
import { getToken, setToken } from '../api/client'
import { nav } from '../lib/router'
import { fmtBytes, fmtUsd } from '../lib/format'

export function Card({ title, children, className = '' }: { title?: string; children: any; className?: string }) {
  return (
    <section class={`card ${className}`}>
      {title && <h3 class="card-title">{title}</h3>}
      {children}
    </section>
  )
}

export function StatCard({
  label,
  value,
  sub,
  accent,
}: {
  label: string
  value: string
  sub?: string
  accent?: string
}) {
  return (
    <div class="stat-card">
      <div class="stat-label">{label}</div>
      <div class="stat-value" style={accent ? `color:${accent}` : undefined}>
        {value}
      </div>
      {sub && <div class="stat-sub">{sub}</div>}
    </div>
  )
}

export function Loading({ text = '加载中…' }: { text?: string }) {
  return <div class="state-box">{text}</div>
}

export function Empty({ text = '暂无数据' }: { text?: string }) {
  return <div class="state-box">{text}</div>
}

export function ErrorBox({ error, onRetry }: { error?: string; onRetry?: () => void }) {
  return (
    <div class="state-box error">
      <span>加载失败：{error || '未知错误'}</span>
      {onRetry && (
        <button type="button" class="btn" onClick={onRetry}>
          重试
        </button>
      )}
    </div>
  )
}

export function Badge({ text, tone }: { text: string; tone?: 'ok' | 'warn' | 'err' | 'muted' }) {
  return <span class={`badge badge-${tone || 'muted'}`}>{text}</span>
}

export function statusTone(status?: string): 'ok' | 'warn' | 'err' | 'muted' {
  switch (status) {
    case 'online':
    case 'active':
    case 'success':
    case 'ended':
      return 'ok'
    case 'degraded':
    case 'interrupted':
      return 'warn'
    case 'offline':
    case 'error':
    case 'missing':
      return 'err'
    default:
      return 'muted'
  }
}

/** 简单数据表格。 */
export function Table({
  columns,
  rows,
  onRowClick,
}: {
  columns: { key: string; label: string; render?: (row: any) => any }[]
  rows: any[]
  onRowClick?: (row: any) => void
}) {
  return (
    <div class="table-wrap">
      <table class="table">
        <thead>
          <tr>
            {columns.map((c) => (
              <th key={c.key}>{c.label}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 && (
            <tr>
              <td colSpan={columns.length} class="table-empty">
                暂无数据
              </td>
            </tr>
          )}
          {rows.map((row, i) => (
            <tr
              key={i}
              onClick={onRowClick ? () => onRowClick(row) : undefined}
              class={onRowClick ? 'clickable' : ''}
            >
              {columns.map((c) => (
                <td key={c.key}>{c.render ? c.render(row) : row[c.key]}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

export function CostCell({ v }: { v?: number | null }) {
  return <span>{fmtUsd(v)}</span>
}

export function TrafficCell({ v, lo, hi }: { v?: number | null; lo?: number | null; hi?: number | null }) {
  return (
    <span title={lo != null && hi != null ? `范围 ${fmtBytes(lo)} ~ ${fmtBytes(hi)}` : undefined}>
      {fmtBytes(v)}
    </span>
  )
}

/** 退出登录按钮。 */
export function LogoutButton() {
  return (
    <button
      type="button"
      class="btn"
      onClick={() => {
        setToken(null)
        nav('login')
      }}
    >
      退出
    </button>
  )
}

/** SSE 订阅：收到事件后调用 onEvent，触发增量刷新。 */
export function useSse(onEvent: (event: string) => void) {
  useEffect(() => {
    const token = getToken()
    if (!token) return
    const es = new EventSource(`/api/v1/stream?token=${encodeURIComponent(token)}`)
    es.onmessage = (ev) => onEvent(ev.type || 'message')
    es.addEventListener('usage.created', () => onEvent('usage.created'))
    es.addEventListener('call.updated', () => onEvent('call.updated'))
    es.addEventListener('session.updated', () => onEvent('session.updated'))
    es.addEventListener('traffic.estimated', () => onEvent('traffic.estimated'))
    es.addEventListener('rollup.updated', () => onEvent('rollup.updated'))
    return () => es.close()
  }, [])
}
