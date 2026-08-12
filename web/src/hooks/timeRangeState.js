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
