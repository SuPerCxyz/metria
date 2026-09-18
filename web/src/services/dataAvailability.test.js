import test from 'node:test'
import assert from 'node:assert/strict'
import { averagePerCall, getVisibleColumns, hasAnyToken, isAvailable } from './dataAvailability.js'

test('availability keeps zero and rejects null or blank values', () => {
  assert.equal(isAvailable(0), true)
  assert.equal(isAvailable('0'), true)
  assert.equal(isAvailable(null), false)
  assert.equal(isAvailable(''), false)
  assert.equal(isAvailable('   '), false)
})

test('token availability distinguishes a real zero from missing usage', () => {
  assert.equal(hasAnyToken({ output_tokens: 0 }), true)
  assert.equal(hasAnyToken({}), false)
})

test('average per call stays unavailable without a valid denominator', () => {
  assert.equal(averagePerCall(100, 4), 25)
  assert.equal(averagePerCall(100, 0), null)
  assert.equal(averagePerCall(null, 4), null)
})

test('optional columns hide only when every loaded row is unavailable', () => {
  const columns = [
    { key: 'name', label: '名称' },
    { key: 'duration', label: '时长', hideWhenEmpty: true },
  ]
  assert.deepEqual(getVisibleColumns(columns, [{ name: 'a', duration: null }]).map((c) => c.key), ['name'])
  assert.deepEqual(getVisibleColumns(columns, [{ name: 'a', duration: 0 }, { name: 'b', duration: null }]).map((c) => c.key), ['name', 'duration'])
  assert.deepEqual(getVisibleColumns(columns, []).map((c) => c.key), ['name', 'duration'])
})
