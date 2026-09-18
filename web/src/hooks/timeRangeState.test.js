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
  assert.deepEqual(previousTimeRange({
    from: '2026-08-10T00:00:00.000Z',
    to: '2026-08-12T00:00:00.000Z',
    timezone: 'Asia/Shanghai',
  }), {
    from: '2026-08-08T00:00:00.000Z',
    to: '2026-08-10T00:00:00.000Z',
    timezone: 'Asia/Shanghai',
  })
})
