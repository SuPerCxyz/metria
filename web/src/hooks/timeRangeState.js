import { quickRange } from '../services/format.js'

export const DEFAULT_PRESET_KEY = 'today'

export function createPresetRange(presetKey, now = new Date()) {
  return { ...quickRange(presetKey, now), presetKey }
}

export function normalizeTimeRange(range) {
  const next = { from: range.from, to: range.to }
  if (range.timezone) next.timezone = range.timezone
  if (range.presetKey) next.presetKey = range.presetKey
  return next
}

export function refreshPresetRange(range, now = new Date()) {
  if (!range?.presetKey) return null

  const next = createPresetRange(range.presetKey, now)
  if (range.timezone) next.timezone = range.timezone
  return next
}

export function isSameTimeRange(left, right) {
  return left?.from === right?.from
    && left?.to === right?.to
    && left?.timezone === right?.timezone
    && left?.presetKey === right?.presetKey
}

// 保证范围至少覆盖 minSpanMs：不足时向前扩展 from，保持 to 与时区不变。
export function withMinimumSpan(range, minSpanMs) {
  const fromMs = new Date(range?.from || '').getTime()
  const toMs = new Date(range?.to || '').getTime()
  if (!Number.isFinite(fromMs) || !Number.isFinite(toMs) || toMs - fromMs >= minSpanMs) return range
  return { ...range, from: new Date(toMs - minSpanMs).toISOString() }
}

function zonedParts(date, timezone) {
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone: timezone,
    weekday: 'short',
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hourCycle: 'h23',
  }).formatToParts(date)
  const value = (type) => parts.find((part) => part.type === type)?.value
  return {
    weekday: value('weekday'),
    year: Number(value('year')),
    month: Number(value('month')),
    day: Number(value('day')),
    hour: Number(value('hour')),
    minute: Number(value('minute')),
    second: Number(value('second')),
  }
}

function zonedWallTimeToUtc(parts, timezone) {
  const wall = Date.UTC(parts.year, parts.month - 1, parts.day, parts.hour || 0, parts.minute || 0, parts.second || 0)
  const offsetParts = zonedParts(new Date(wall), timezone)
  const offsetWall = Date.UTC(offsetParts.year, offsetParts.month - 1, offsetParts.day, offsetParts.hour, offsetParts.minute, offsetParts.second)
  return new Date(wall - (offsetWall - wall))
}

/** 当前自然周：周一 00:00 至 now，按展示时区计算。 */
export function currentWeekRange(timezone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC', now = new Date()) {
  const current = zonedParts(now, timezone)
  const weekday = { Sun: 0, Mon: 1, Tue: 2, Wed: 3, Thu: 4, Fri: 5, Sat: 6 }[current.weekday] ?? 0
  const mondayOffset = (weekday + 6) % 7
  const monday = new Date(Date.UTC(current.year, current.month - 1, current.day - mondayOffset))
  return {
    from: zonedWallTimeToUtc({
      year: monday.getUTCFullYear(),
      month: monday.getUTCMonth() + 1,
      day: monday.getUTCDate(),
      hour: 0,
      minute: 0,
      second: 0,
    }, timezone).toISOString(),
    to: new Date(now).toISOString(),
    timezone,
    presetKey: 'current-week',
  }
}

// 是否为本地午夜（「今天/昨天」等自然日类范围的起点）。
function isLocalMidnight(date) {
  return date.getHours() === 0
    && date.getMinutes() === 0
    && date.getSeconds() === 0
    && date.getMilliseconds() === 0
}

// 日历日平移：用 setDate(getDate() + days) 而非回退固定毫秒，跨 DST 也保持时钟位置一致。
function shiftCalendarDays(date, days) {
  const next = new Date(date.getTime())
  next.setDate(next.getDate() + days)
  return next
}

// 部分日范围跨度上限：起点为本地午夜且跨度 < 24h 才算「今天」类日历型部分日范围。
const PARTIAL_DAY_SPAN_MS = 24 * 60 * 60 * 1000

// 生成当前范围之前的对比周期，用于总览 KPI 环比。
// 命中「昨日同时段」需同时满足：起点为本地午夜 + 跨度 < 24h（部分日范围，如「今天」），
// 此时 from/to 各回退 1 个日历日，时钟位置与当前窗口一致；
// 其余范围（「本周」周一 00:00、多日午夜起点、非午夜起点）保持紧邻等长前窗 [from - span, from)。
// 跨度恰为 24h 时两分支结果一致（紧邻前窗 ≡ 减一天）。空/非法范围返回 null。
export function previousTimeRange(range) {
  const from = new Date(range?.from || '')
  const to = new Date(range?.to || '')
  const fromMs = from.getTime()
  const toMs = to.getTime()
  if (!Number.isFinite(fromMs) || !Number.isFinite(toMs) || toMs <= fromMs) return null
  const span = toMs - fromMs
  const sameClockPreviousDay = isLocalMidnight(from) && span < PARTIAL_DAY_SPAN_MS
  return {
    from: (sameClockPreviousDay ? shiftCalendarDays(from, -1) : new Date(fromMs - span)).toISOString(),
    to: (sameClockPreviousDay ? shiftCalendarDays(to, -1) : new Date(fromMs)).toISOString(),
    ...(range.timezone ? { timezone: range.timezone } : {}),
  }
}
