import assert from 'node:assert/strict'
import test from 'node:test'
import { compareSortValues, sortItems } from './sorting.js'

test('keeps unavailable values last in both directions', () => {
  const items = [{ id: 'missing' }, { id: 'two', value: 2 }, { id: 'one', value: 1 }]

  assert.deepEqual(sortItems(items, (item) => item.value, 1).map((item) => item.id), ['one', 'two', 'missing'])
  assert.deepEqual(sortItems(items, (item) => item.value, -1).map((item) => item.id), ['two', 'one', 'missing'])
})

test('sorts numeric and natural text values', () => {
  assert.equal(compareSortValues(2, 10), -8)
  assert.equal(compareSortValues('model-2', 'model-10'), -1)
})

test('keeps equal values stable', () => {
  const items = [{ id: 'first', value: 1 }, { id: 'second', value: 1 }, { id: 'third', value: 2 }]
  assert.deepEqual(sortItems(items, (item) => item.value).map((item) => item.id), ['first', 'second', 'third'])
})

test('sorts derived totals and ISO timestamps before display formatting', () => {
  const items = [
    { id: 'later', started_at: '2026-09-18T12:00:00Z', input: 2, cache: 8 },
    { id: 'earlier', started_at: '2026-09-18T10:00:00Z', input: 4, cache: 1 },
  ]

  assert.deepEqual(sortItems(items, (item) => item.input + item.cache).map((item) => item.id), ['earlier', 'later'])
  assert.deepEqual(sortItems(items, (item) => item.started_at).map((item) => item.id), ['earlier', 'later'])
})
