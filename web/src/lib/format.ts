/** 格式化工具。 */

export function fmtBytes(b: number | null | undefined): string {
  if (b == null || b === 0) return '0 B'
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB']
  let v = b
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`
}

export function fmtTokens(t: number | null | undefined): string {
  if (t == null) return '—'
  if (t === 0) return '0'
  if (t >= 1_000_000) return `${(t / 1_000_000).toFixed(2)}M`
  if (t >= 1000) return `${(t / 1000).toFixed(1)}k`
  return String(t)
}

export function fmtUsd(micro: number | null | undefined): string {
  if (micro == null || micro === 0) return '$0.00'
  return `$${(micro / 1_000_000).toFixed(4)}`
}

export function fmtDateTime(iso: string | null | undefined): string {
  if (!iso) return '—'
  const d = new Date(iso)
  return d.toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function fmtDuration(start: string | null | undefined, end: string | null | undefined): string {
  if (!start) return '—'
  const s = new Date(start).getTime()
  const e = end ? new Date(end).getTime() : Date.now()
  const ms = Math.max(0, e - s)
  if (ms < 1000) return `${ms}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  return `${Math.floor(ms / 60_000)}m${Math.floor((ms % 60_000) / 1000)}s`
}

export function pct(part: number, total: number): number {
  if (total <= 0) return 0
  return Math.round((part / total) * 100)
}
