const DAY_MS = 24 * 60 * 60 * 1000

export const MAX_CHART_POINTS = 240

export function downsampleIndices(length, maxPoints = MAX_CHART_POINTS) {
  if (length <= maxPoints) return Array.from({ length }, (_, index) => index)
  const step = Math.ceil(length / maxPoints)
  const indices = []
  for (let index = 0; index < length; index += step) indices.push(index)
  return indices
}

export function isRangeLongerThanDay(range) {
  if (!range?.from || !range?.to) return true
  const from = new Date(range.from).getTime()
  const to = new Date(range.to).getTime()
  if (!Number.isFinite(from) || !Number.isFinite(to)) return true
  return to - from > DAY_MS
}

export function formatTimeLabel(value, withSeconds = false, includeDate = true) {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}T/.test(value)) return value
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  const pad = (number) => String(number).padStart(2, '0')
  const time = `${pad(date.getHours())}:${pad(date.getMinutes())}${withSeconds ? `:${pad(date.getSeconds())}` : ''}`
  if (!includeDate) return time
  return withSeconds
    ? `${date.getFullYear()}/${pad(date.getMonth() + 1)}/${pad(date.getDate())} ${time}`
    : `${pad(date.getMonth() + 1)}/${pad(date.getDate())} ${time}`
}
