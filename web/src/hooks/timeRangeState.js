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

// 生成当前范围之前的等长周期，用于总览 KPI 对比。
export function previousTimeRange(range) {
  const from = new Date(range?.from || '')
  const to = new Date(range?.to || '')
  const fromMs = from.getTime()
  const toMs = to.getTime()
  if (!Number.isFinite(fromMs) || !Number.isFinite(toMs) || toMs <= fromMs) return null
  const span = toMs - fromMs
  return {
    from: new Date(fromMs - span).toISOString(),
    to: new Date(fromMs).toISOString(),
    ...(range.timezone ? { timezone: range.timezone } : {}),
  }
}
