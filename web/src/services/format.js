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

/**
 * 统一 Token 总计口径（v2）：input + output + reasoning + cache_read + cache_write。
 * 全站「总 Token / Token 列」必须使用本函数；等价于 ccswitch 用量面板的消耗总量口径。
 * v1（2026-09-15 之前）不含缓存读写，跨版本对比时数值不可比。
 */
export function sumTokens(o) {
  if (!o) return 0
  return (
    (o.input_tokens ?? 0) +
    (o.output_tokens ?? 0) +
    (o.reasoning_tokens ?? 0) +
    (o.cache_read_tokens ?? 0) +
    (o.cache_write_tokens ?? 0)
  )
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

/** Agent 地址：隐藏传输协议，保留主机与端口。 */
export function fmtAgentAddress(value) {
  return String(value || '').replace(/^https?:\/\//i, '').replace(/\/+$/, '')
}

/** 时长：680 毫秒；4.8 秒；12 分 36 秒；超过 1 小时自动折算。 */
export function fmtDuration(ms) {
  if (ms === null || ms === undefined || Number.isNaN(ms)) return '—'
  const v = Number(ms)
  if (v < 1000) return `${Math.round(v)} 毫秒`
  if (v < 60000 && Math.round(v / 1000) < 60) return `${(v / 1000).toFixed(1)} 秒`
  const totalSeconds = Math.round(v / 1000)
  const min = Math.floor(totalSeconds / 60)
  const sec = totalSeconds % 60
  if (min < 60) return `${min} 分 ${sec} 秒`
  const hours = Math.floor(min / 60)
  const minutes = min % 60
  if (hours < 24) return minutes > 0 ? `${hours} 小时 ${minutes} 分` : `${hours} 小时`
  const days = Math.floor(hours / 24)
  const remainderHours = hours % 24
  return remainderHours > 0 ? `${days} 天 ${remainderHours} 小时` : `${days} 天`
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

/** 当前值相对上一等长周期的百分比变化；上一周期为 0 时不可比较。 */
export function percentChange(current, previous) {
  const currentValue = Number(current)
  const previousValue = Number(previous)
  if (!Number.isFinite(currentValue) || !Number.isFinite(previousValue) || previousValue === 0) return null
  return ((currentValue - previousValue) / Math.abs(previousValue)) * 100
}

/** KPI 对比标签：+12.3% / -4.0%。 */
export function fmtChange(current, previous) {
  const change = percentChange(current, previous)
  if (change === null) return null
  return `${change >= 0 ? '+' : ''}${change.toFixed(1)}%`
}

/** 返回 MetricCard 使用的涨跌语义；inverse 用于错误率等“越低越好”指标。 */
export function changeTone(current, previous, inverse = false) {
  const change = percentChange(current, previous)
  if (change === null || change === 0) return 'neutral'
  const positive = inverse ? change < 0 : change > 0
  return positive ? 'up' : 'down'
}

/**
 * 缓存命中率：cache_read / (input + cache_write + cache_read)。
 * 返回 0-100 的百分比数值；无缓存数据或分母为 0 返回 null（前端显示「—」，不硬造）。
 */
export function cacheHitRate(o) {
  if (!o) return null
  const input = Number(o.input_tokens ?? 0)
  const cr = Number(o.cache_read_tokens ?? 0)
  const cw = Number(o.cache_write_tokens ?? 0)
  const cacheable = input + cw + cr
  if (cacheable <= 0 || (cr <= 0 && cw <= 0)) return null
  return (cr / cacheable) * 100
}

/** ISO 时间 → 用户时区本地 24 小时格式。 */
export function fmtDateTime(iso, opts = {}) {
  if (!iso) return '—'
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return String(iso)
  const options = {
    year: 'numeric', month: '2-digit', day: '2-digit',
    hour: '2-digit', minute: '2-digit',
    hour12: false,
    ...opts,
  }
  const parts = new Intl.DateTimeFormat('zh-CN', options).formatToParts(d)
  const valueOf = (type) => parts.find((part) => part.type === type)?.value || ''
  const date = `${valueOf('year')}-${valueOf('month')}-${valueOf('day')}`
  const time = `${valueOf('hour')}:${valueOf('minute')}`
  return options.second ? `${date} ${time}:${valueOf('second')}` : `${date} ${time}`
}

/** ISO 时间 → 日期（无时间） */
export function fmtDate(iso) {
  const formatted = fmtDateTime(iso)
  return formatted === '—' ? formatted : formatted.slice(0, 10)
}

/** 相对时间：刚刚 / 几分钟前 / 几小时前。 */
export function fmtRelative(iso) {
  if (!iso) return '—'
  const t = new Date(iso).getTime()
  const diff = Date.now() - t
  if (Number.isNaN(t)) return String(iso)
  const s = Math.floor(diff / 1000)
  if (s < 10) return '刚刚'
  if (s < 60) return `${s} 秒前`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m} 分钟前`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h} 小时前`
  const d = Math.floor(h / 24)
  return `${d} 天前`
}

/** 会话标题：替换客户端生成的英文时间戳回退标题。 */
export function fmtSessionTitle(title, startedAt) {
  const value = String(title || '').trim()
  if (value && !/^New session\s*-\s*\d{4}-\d{2}-\d{2}T/.test(value)) return value
  const time = fmtDateTime(startedAt)
  return time === '—' ? '未命名会话' : `未命名会话 · ${time}`
}

/** 计算时间范围（快捷项）；now 参数用于刷新时重算与确定性测试。 */
export function quickRange(key, nowValue = new Date()) {
  const now = new Date(nowValue)
  const to = new Date(now)
  const from = new Date(now)
  switch (key) {
    case 'today': from.setHours(0, 0, 0, 0); break
    case 'yesterday': { from.setDate(now.getDate() - 1); from.setHours(0, 0, 0, 0); to.setDate(now.getDate() - 1); to.setHours(23, 59, 59, 999); break }
    case '1h': from.setTime(now.getTime() - 3600 * 1000); break
    case '3h': from.setTime(now.getTime() - 3 * 3600 * 1000); break
    case '6h': from.setTime(now.getTime() - 6 * 3600 * 1000); break
    case '12h': from.setTime(now.getTime() - 12 * 3600 * 1000); break
    case '24h': from.setTime(now.getTime() - 24 * 3600 * 1000); break
    case '7d': from.setTime(now.getTime() - 7 * 24 * 3600 * 1000); break
    case '14d': from.setTime(now.getTime() - 14 * 24 * 3600 * 1000); break
    case '30d': from.setTime(now.getTime() - 30 * 24 * 3600 * 1000); break
    default: from.setTime(now.getTime() - 7 * 24 * 3600 * 1000)
  }
  return { from: from.toISOString(), to: to.toISOString(), presetKey: key }
}

/** 状态色映射 */
export function statusTone(status) {
  const s = String(status || '').toLowerCase()
  if (['active', 'online', 'success', 'ok', 'healthy'].includes(s)) return 'success'
  if (['idle', 'paused', 'waiting'].includes(s)) return 'warning'
  if (['error', 'failed', 'offline', 'unknown', 'unavailable', 'fatal'].includes(s)) return 'danger'
  if (['warning', 'partial', 'degraded', 'skipped'].includes(s)) return 'warning'
  return 'muted'
}
