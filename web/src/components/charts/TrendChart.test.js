import test from 'node:test'
import assert from 'node:assert/strict'
import { downsampleIndices, formatTimeLabel, isRangeLongerThanDay } from './trendChartLabels.js'

test('long-range chart downsampling keeps the same deterministic points', () => {
  assert.deepEqual(downsampleIndices(337), Array.from({ length: 169 }, (_, i) => i * 2))
})

test('ISO labels are compact on the axis and complete in the tooltip', () => {
  const iso = new Date(2026, 7, 13, 9, 5, 7).toISOString()

  assert.equal(formatTimeLabel(iso), '08/13 09:05')
  assert.equal(formatTimeLabel(iso, true), '2026/08/13 09:05:07')
})

test('axis labels include the date only for ranges longer than one day', () => {
  const iso = new Date(2026, 7, 13, 9, 5, 7).toISOString()

  assert.equal(formatTimeLabel(iso, false, false), '09:05')
  assert.equal(formatTimeLabel(iso, false, true), '08/13 09:05')
  assert.equal(isRangeLongerThanDay({ from: iso, to: new Date(new Date(iso).getTime() + 7 * 24 * 60 * 60 * 1000).toISOString() }), true)
  assert.equal(isRangeLongerThanDay({ from: iso, to: new Date(new Date(iso).getTime() + 24 * 60 * 60 * 1000).toISOString() }), false)
})

test('non-time category labels stay unchanged', () => {
  assert.equal(formatTimeLabel('gpt-5.6-sol'), 'gpt-5.6-sol')
  assert.equal(formatTimeLabel('not-an-iso-date'), 'not-an-iso-date')
})
