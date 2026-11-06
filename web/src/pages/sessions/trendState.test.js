import assert from 'node:assert/strict'
import test from 'node:test'
import { trendViewState } from './trendState.js'

test('loading wins over empty and error', () => {
  assert.equal(trendViewState({ loading: true, error: null, labels: [] }), 'loading')
})

test('error is not rendered as empty data', () => {
  assert.equal(
    trendViewState({ loading: false, error: new Error('stats failed'), labels: [] }),
    'error',
  )
})

test('empty is only used after a successful empty response', () => {
  assert.equal(trendViewState({ loading: false, error: null, labels: [] }), 'empty')
  assert.equal(trendViewState({ loading: false, error: null, labels: ['a'] }), 'chart')
})
