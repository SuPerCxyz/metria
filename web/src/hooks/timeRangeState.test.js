import assert from 'node:assert/strict'
import test from 'node:test'
import {
  createPresetRange,
  isSameTimeRange,
  normalizeTimeRange,
  refreshPresetRange,
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
