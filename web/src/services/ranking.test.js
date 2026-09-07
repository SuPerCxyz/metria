import assert from 'node:assert/strict'
import test from 'node:test'
import { sortRankingItems } from './ranking.js'

test('sorts rankings by value before applying the limit and keeps ties stable', () => {
  const items = [
    { id: 'small', value: 2 },
    { id: 'tie-a', value: 5 },
    { id: 'large', value: 10 },
    { id: 'tie-b', value: 5 },
  ]

  assert.deepEqual(
    sortRankingItems(items, 'value', 3).map((item) => item.id),
    ['large', 'tie-a', 'tie-b'],
  )
  assert.deepEqual(items.map((item) => item.id), ['small', 'tie-a', 'large', 'tie-b'])
})
