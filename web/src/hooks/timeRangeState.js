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
