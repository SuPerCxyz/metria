import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const source = readFileSync(new URL('./usePagedSessionList.js', import.meta.url), 'utf8')

test('refresh event resets the cursor and reloads the first page', () => {
  assert.ok(source.includes("addEventListener('metria:refresh'"))
  assert.ok(source.includes("removeEventListener('metria:refresh'"))
  assert.ok(source.includes('cursorRef.current = null'))
  assert.ok(source.includes('fetchPage(null, true)'))
})
