// 统一数据格式化工具：数字、Token、费用、流量、时长、百分比、时间。

/** 数字缩写：1284 → 1,284；12.6K；8.42M；1.37B */
export function fmtNumber(n) {
  if (n === null || n === undefined || Number.isNaN(n)) return '—'
  const v = Number(n)
  const abs = Math.abs(v)
  if (abs >= 1e12) return `${(v / 1e12).toFixed(2)}T`
  if (abs >= 1e9) return `${(v / 1e9).toFixed(2)}B`
  if (abs >= 1e6) return `${(v / 1e6).toFixed(2)}M`
  if (abs >= 1e3) return `${(v / 1e3).toFixed(1)}K`
  return v.toLocaleString('en-US')
}

/** Token：12.8M tokens */
export function fmtTokens(t) {
  if (t === null || t === undefined || Number.isNaN(t)) return '—'
  return `${fmtNumber(t)} tokens`
}

/** Token 简写（表格用）：12.8M */
export function fmtTokensShort(t) {
  if (t === null || t === undefined || Number.isNaN(t)) return '—'
  return fmtNumber(t)
}

/** 费用：微美元 → $12.48 */
export function fmtUsd(micro) {
  if (micro === null || micro === undefined || Number.isNaN(micro)) return '—'
  return `$${(Number(micro) / 1e6).toFixed(2)}`
}

/** 费用（多位小数，详情用） */
export function fmtUsdPrecise(micro) {
  if (micro === null || micro === undefined || Number.isNaN(micro)) return '—'
  return `$${(Number(micro) / 1e6).toFixed(6)}`
}

/** 流量：824 KB；128 MB；12.4 GB */
export function fmtBytes(b) {
  if (b === null || b === undefined || Number.isNaN(b)) return '—'
  const v = Number(b)
  const abs = Math.abs(v)
  if (abs >= 1e12) return `${(v / 1e12).toFixed(2)} TB`
  if (abs >= 1e9) return `${(v / 1e9).toFixed(1)} GB`
  if (abs >= 1e6) return `${(v / 1e6).toFixed(0)} MB`
  if (abs >= 1e3) return `${(v / 1e3).toFixed(0)} KB`
  return `${v} B`
}

/** 时长：680 ms；4.8 s；12 min 36 s */
export function fmtDuration(ms) {
  if (ms === null || ms === undefined || Number.isNaN(ms)) return '—'
  const v = Number(ms)
  if (v < 1000) return `${Math.round(v)} ms`
  if (v < 60000) return `${(v / 1000).toFixed(1)} s`
  const min = Math.floor(v / 60000)
  const sec = Math.round((v % 60000) / 1000)
  return `${min} min ${sec} s`
}

/** 百分比：82.4% */
export function fmtPct(p) {
  if (p === null || p === undefined || Number.isNaN(p)) return '—'
  return `${(Number(p) * 100).toFixed(1)}%`
}

/** 百分比（已按 0-100 传入）：82.4% */
export function fmtPct100(p) {
  if (p === null || p === undefined || Number.isNaN(p)) return '—'
  return `${Number(p).toFixed(1)}%`
}

/** ISO 时间 → 用户时区本地格式 */
export function fmtDateTime(iso, opts = {}) {
  if (!iso) return '—'
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return String(iso)
  const o = {
    year: 'numeric', month: '2-digit', day: '2-digit',
    hour: '2-digit', minute: '2-digit',
    ...opts,
  }
  return d.toLocaleString([], o)
}

/** ISO 时间 → 日期（无时间） */
export function fmtDate(iso) {
  if (!iso) return '—'
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return String(iso)
  return d.toLocaleDateString()
}

/** 相对时间：几分钟前 / 几小时前 */
export function fmtRelative(iso) {
  if (!iso) return '—'
  const t = new Date(iso).getTime()
  const diff = Date.now() - t
  if (Number.isNaN(t)) return String(iso)
  const s = Math.floor(diff / 1000)
  if (s < 60) return `${s}s ago`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m ago`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h}h ago`
  const d = Math.floor(h / 24)
  return `${d}d ago`
}

/** 计算时间范围（快捷项） */
export function quickRange(key) {
  const now = new Date()
  const to = new Date(now)
  const from = new Date(now)
  switch (key) {
    case 'today': from.setHours(0, 0, 0, 0); break
    case 'yesterday': { from.setDate(now.getDate() - 1); from.setHours(0, 0, 0, 0); to.setDate(now.getDate() - 1); to.setHours(23, 59, 59, 999); break }
    case '24h': from.setTime(now.getTime() - 24 * 3600 * 1000); break
    case '7d': from.setTime(now.getTime() - 7 * 24 * 3600 * 1000); break
    case '30d': from.setTime(now.getTime() - 30 * 24 * 3600 * 1000); break
    default: from.setTime(now.getTime() - 7 * 24 * 3600 * 1000)
  }
  return { from: from.toISOString(), to: to.toISOString() }
}

/** 状态色映射 */
export function statusTone(status) {
  const s = String(status || '').toLowerCase()
  if (['active', 'online', 'success', 'ok', 'healthy'].includes(s)) return 'success'
  if (['error', 'failed', 'offline', 'unknown', 'unavailable', 'fatal'].includes(s)) return 'danger'
  if (['warning', 'partial', 'degraded', 'skipped'].includes(s)) return 'warning'
  return 'muted'
}
