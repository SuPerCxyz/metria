import test from 'node:test'
import assert from 'node:assert/strict'
import { formatTimeLabel } from './trendChartLabels.js'

test('ISO labels are compact on the axis and complete in the tooltip', () => {
  const iso = new Date(2026, 7, 13, 9, 5, 7).toISOString()

  assert.equal(formatTimeLabel(iso), '08/13 09:05')
  assert.equal(formatTimeLabel(iso, true), '2026/08/13 09:05:07')
})

test('non-time category labels stay unchanged', () => {
  assert.equal(formatTimeLabel('gpt-5.6-sol'), 'gpt-5.6-sol')
  assert.equal(formatTimeLabel('not-an-iso-date'), 'not-an-iso-date')
})
