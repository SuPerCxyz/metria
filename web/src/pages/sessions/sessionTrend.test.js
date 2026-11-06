import assert from 'node:assert/strict'
import test from 'node:test'
import { buildSessionTrend } from './sessionTrend.js'

const points = [
  {
    bucket: '2026-08-10T10:00:00Z',
    input_tokens: 1,
    output_tokens: 2,
    reasoning_tokens: 3,
    cache_read_tokens: 4,
    cache_write_tokens: 5,
    cost_micro_usd: 99,
    duration_ms: 400,
    avg_duration_ms: 200,
  },
  {
    bucket: '2026-08-10T11:00:00Z',
    input_tokens: 0,
    output_tokens: 0,
    cost_micro_usd: null,
    duration_ms: null,
    avg_duration_ms: null,
  },
]

test('latency uses average duration, not the per-bucket sum', () => {
  const trend = buildSessionTrend(points, 'latency')
  assert.deepEqual(trend.labels, points.map((point) => point.bucket))
  assert.deepEqual(trend.values, [200, null])
})

test('tokens and cost keep their existing semantics', () => {
  assert.deepEqual(buildSessionTrend(points, 'tokens').values, [15, 0])
  assert.deepEqual(buildSessionTrend(points, 'cost').values, [99, null])
})

test('missing series falls back to empty', () => {
  assert.deepEqual(buildSessionTrend(undefined, 'latency'), { labels: [], values: [] })
})
