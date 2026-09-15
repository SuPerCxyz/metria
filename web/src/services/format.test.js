import assert from 'node:assert/strict'
import test from 'node:test'
import { changeTone, fmtAgentAddress, fmtChange, fmtDateTime, fmtDuration, fmtRelative, fmtSessionTitle, fmtUsd, percentChange, sumTokens } from './format.js'

test('excludes cache tokens from the total while keeping reasoning tokens', () => {
  assert.equal(sumTokens({
    input_tokens: 100,
    output_tokens: 50,
    reasoning_tokens: 10,
    cache_read_tokens: 900,
    cache_write_tokens: 20,
  }), 160)
})

test('formats timestamps with a stable 24-hour shape', () => {
  const value = fmtDateTime('2026-08-21T13:47:05Z', { second: '2-digit' })
  assert.match(value, /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/)
  assert.doesNotMatch(value, /AM|PM/)
})

test('percent change formats positive, negative, and incomparable values', () => {
  assert.equal(percentChange(125, 100), 25)
  assert.equal(fmtChange(96, 100), '-4.0%')
  assert.equal(fmtChange(10, 0), null)
  assert.equal(changeTone(90, 100, true), 'up')
})

test('formats missing ranking fees as unavailable while preserving free zero', () => {
  assert.equal(fmtUsd(null), '—')
  assert.equal(fmtUsd(0), '$0.00')
})

test('formats durations with Chinese units and hour roll-up', () => {
  assert.equal(fmtDuration(680), '680 毫秒')
  assert.equal(fmtDuration(8 * 60 * 1000 + 7 * 1000), '8 分 7 秒')
  assert.equal(fmtDuration((21 * 60 + 53) * 60 * 1000), '21 小时 53 分')
})

test('formats relative time and unnamed session fallbacks in Chinese', () => {
  assert.match(fmtRelative(new Date().toISOString()), /刚刚|秒前/)
  assert.equal(fmtSessionTitle('New session - 2026-08-21T02:50:43.684Z', '2026-08-21T02:50:43.684Z').startsWith('未命名会话 · '), true)
  assert.equal(fmtSessionTitle('修复登录问题', '2026-08-21T02:50:43.684Z'), '修复登录问题')
  assert.equal(fmtAgentAddress('http://agent.example.com:8090/'), 'agent.example.com:8090')
})
