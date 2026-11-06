import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const source = readFileSync(new URL('./TimeRangePicker.jsx', import.meta.url), 'utf8')

test('lazy calendar is isolated behind an inline error boundary with retry', () => {
  assert.ok(source.includes('InlineErrorBoundary'))
  assert.ok(source.includes('CalendarSlot'))
  assert.ok(source.includes('日历加载失败'))
  assert.ok(source.includes('刷新重试'))
})
