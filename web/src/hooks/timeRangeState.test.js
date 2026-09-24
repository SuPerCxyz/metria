import assert from 'node:assert/strict'
import test from 'node:test'
import {
  createPresetRange,
  currentWeekRange,
  isSameTimeRange,
  normalizeTimeRange,
  previousTimeRange,
  refreshPresetRange,
  withMinimumSpan,
} from './timeRangeState.js'

const NOW = new Date('2026-08-12T10:30:00.000Z')

test('today ends at now and starts at local midnight', () => {
  const expectedFrom = new Date(NOW)
  expectedFrom.setHours(0, 0, 0, 0)

  assert.deepEqual(createPresetRange('today', NOW), {
    from: expectedFrom.toISOString(),
    to: NOW.toISOString(),
    presetKey: 'today',
  })
})

test('rolling presets are recalculated backwards from now', () => {
  for (const [key, durationMs] of [['1h', 3_600_000], ['24h', 86_400_000], ['7d', 604_800_000]]) {
    const range = createPresetRange(key, NOW)
    assert.equal(range.to, NOW.toISOString())
    assert.equal(new Date(range.to).getTime() - new Date(range.from).getTime(), durationMs)
  }
})

test('preset refresh preserves its identity and timezone', () => {
  const current = { ...createPresetRange('7d', new Date('2026-08-11T10:30:00.000Z')), timezone: 'Asia/Shanghai' }
  const refreshed = refreshPresetRange(current, NOW)

  assert.equal(refreshed.presetKey, '7d')
  assert.equal(refreshed.timezone, 'Asia/Shanghai')
  assert.equal(refreshed.to, NOW.toISOString())
  assert.equal(isSameTimeRange(current, refreshed), false)
})

test('custom ranges stay fixed on refresh', () => {
  const custom = normalizeTimeRange({
    from: '2026-08-01T01:00:00.000Z',
    to: '2026-08-02T02:00:00.000Z',
  })

  assert.equal(refreshPresetRange(custom, NOW), null)
})

test('withMinimumSpan extends a short range backwards and keeps its end and timezone', () => {
  assert.deepEqual(withMinimumSpan({
    from: '2026-08-12T09:00:00.000Z',
    to: '2026-08-12T10:00:00.000Z',
    timezone: 'Asia/Shanghai',
  }, 604_800_000), {
    from: '2026-08-05T10:00:00.000Z',
    to: '2026-08-12T10:00:00.000Z',
    timezone: 'Asia/Shanghai',
  })
})

test('withMinimumSpan leaves ranges at or above the minimum untouched', () => {
  const month = { from: '2026-07-01T00:00:00.000Z', to: '2026-08-01T00:00:00.000Z' }
  assert.equal(withMinimumSpan(month, 604_800_000), month)
})

test('currentWeekRange starts at Monday midnight in the display timezone', () => {
  assert.deepEqual(currentWeekRange('Asia/Shanghai', NOW), {
    from: '2026-08-09T16:00:00.000Z',
    to: '2026-08-12T10:30:00.000Z',
    timezone: 'Asia/Shanghai',
    presetKey: 'current-week',
  })
})

test('previous range is an equal-length period immediately before current range', () => {
  // 起点故意避开本地午夜：本地午夜起点改走「昨日同时段」分支，由下方用例覆盖。
  // 否则 CI（UTC）下 00:00Z 恰为本地午夜，会改变本用例命中的分支。
  assert.deepEqual(previousTimeRange({
    from: '2026-08-10T09:30:00.000Z',
    to: '2026-08-12T09:30:00.000Z',
    timezone: 'Asia/Shanghai',
  }), {
    from: '2026-08-08T09:30:00.000Z',
    to: '2026-08-10T09:30:00.000Z',
    timezone: 'Asia/Shanghai',
  })
})

test('previous range for a local-midnight start is the same clock time on the previous calendar day', () => {
  const from = new Date(2026, 9, 1, 0, 0, 0, 0) // 本地午夜（10-01 00:00）
  const to = new Date(2026, 9, 1, 14, 20, 0, 0) // 跨度 14h20m < 24h：部分日范围
  assert.ok(to.getTime() - from.getTime() < 24 * 60 * 60 * 1000)
  const prev = previousTimeRange({
    from: from.toISOString(),
    to: to.toISOString(),
    timezone: 'Asia/Shanghai',
  })

  const previousDayFrom = new Date(from)
  previousDayFrom.setDate(previousDayFrom.getDate() - 1)
  const previousDayTo = new Date(to) // dateNminus1SameClock：昨日同一时刻
  previousDayTo.setDate(previousDayTo.getDate() - 1)

  assert.equal(prev.from, previousDayFrom.toISOString())
  assert.equal(prev.to, previousDayTo.toISOString())
  // 与当前窗口等长
  assert.equal(
    new Date(prev.to).getTime() - new Date(prev.from).getTime(),
    to.getTime() - from.getTime(),
  )
  assert.equal(prev.timezone, 'Asia/Shanghai')
})

test('previous range for a non-midnight start stays the adjacent equal-length period', () => {
  const from = new Date(2026, 9, 1, 14, 30, 45, 0) // 14:30:45，非本地午夜
  const to = new Date(2026, 9, 1, 18, 0, 0, 0)
  const span = to.getTime() - from.getTime()
  const prev = previousTimeRange({
    from: from.toISOString(),
    to: to.toISOString(),
    timezone: 'Asia/Shanghai',
  })

  assert.notEqual(from.getHours(), 0)
  assert.equal(prev.to, from.toISOString())
  assert.equal(prev.from, new Date(from.getTime() - span).toISOString())
  assert.equal(new Date(prev.to).getTime() - new Date(prev.from).getTime(), span)
  assert.equal(prev.timezone, 'Asia/Shanghai')
})

test('previous range returns null for empty or invalid ranges', () => {
  assert.equal(previousTimeRange(null), null)
  assert.equal(previousTimeRange(undefined), null)
  assert.equal(previousTimeRange({}), null)
  assert.equal(previousTimeRange({ from: 'not-a-date', to: '2026-08-12T00:00:00.000Z' }), null)
  assert.equal(previousTimeRange({ from: '2026-08-12T00:00:00.000Z', to: '2026-08-12T00:00:00.000Z' }), null)
  assert.equal(previousTimeRange({ from: '2026-08-12T00:00:00.000Z', to: '2026-08-11T00:00:00.000Z' }), null)
})

test('previous range for a 24h local-midnight start uses the adjacent window', () => {
  const from = new Date(2026, 9, 1, 0, 0, 0, 0) // 本地午夜
  const to = new Date(2026, 9, 2, 0, 0, 0, 0) // 跨度恰好 24h：非部分日
  const span = to.getTime() - from.getTime()
  assert.equal(span, 24 * 60 * 60 * 1000) // 用例前提：整 24h
  const prev = previousTimeRange({
    from: from.toISOString(),
    to: to.toISOString(),
    timezone: 'Asia/Shanghai',
  })
  // 不命中午夜分支 → 紧邻等长前窗；跨度恰为 24h 时与「减一天」结果一致，故断言共同结果
  assert.equal(prev.to, from.toISOString())
  assert.equal(prev.from, new Date(from.getTime() - span).toISOString())
  assert.equal(prev.timezone, 'Asia/Shanghai')
})

test('previous range for a multi-day local-midnight start stays adjacent instead of shifting a day', () => {
  const from = new Date(2026, 9, 1, 0, 0, 0, 0) // 本地午夜（10-01 00:00）
  const to = new Date(2026, 9, 4, 12, 0, 0, 0) // 跨度 84h > 24h：多日范围
  const span = to.getTime() - from.getTime()
  assert.equal(from.getHours(), 0) // 用例前提：起点为本地午夜
  assert.ok(span > 24 * 60 * 60 * 1000) // 用例前提：跨度 ≥ 24h
  const prev = previousTimeRange({
    from: from.toISOString(),
    to: to.toISOString(),
    timezone: 'Asia/Shanghai',
  })
  const shiftedTo = new Date(to)
  shiftedTo.setDate(shiftedTo.getDate() - 1)
  assert.equal(prev.to, from.toISOString()) // 紧邻前窗
  assert.equal(prev.from, new Date(from.getTime() - span).toISOString())
  // 与「昨日同时段（减一天）」可区分：证明没有命中日历日平移分支
  assert.notEqual(prev.to, shiftedTo.toISOString())
  assert.equal(prev.timezone, 'Asia/Shanghai')
})

test('previous range for the current-week range stays adjacent', () => {
  const week = currentWeekRange('Asia/Shanghai', NOW) // 周一 00:00 → now
  const span = new Date(week.to).getTime() - new Date(week.from).getTime()
  assert.ok(span > 24 * 60 * 60 * 1000) // 跨度 ≥ 24h：非部分日
  const prev = previousTimeRange(week)
  assert.equal(prev.to, week.from) // 紧邻前窗，未减一天
  assert.equal(prev.from, new Date(new Date(week.from).getTime() - span).toISOString())
  assert.equal(prev.timezone, 'Asia/Shanghai')
})
