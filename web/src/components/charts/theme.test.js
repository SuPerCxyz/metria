// 色彩标准守卫：chart-theme.json 是图表颜色的唯一来源（见 docs/design-system.md）。

import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'

const theme = JSON.parse(
  fs.readFileSync(new URL('../../../chart-theme.json', import.meta.url), 'utf8'),
)

const TOKEN_STANDARD = {
  input: '#6366f1',
  output: '#10b981',
  cacheRead: '#f59e0b',
  cacheWrite: '#06b6d4',
  reasoning: '#8b5cf6',
}

test('token categories keep the documented standard colors', () => {
  assert.deepEqual(theme.colors.token, TOKEN_STANDARD)
})

test('every fixed color comes from the shared palette', () => {
  assert.ok(Array.isArray(theme.palette) && theme.palette.length >= 5)
  for (const [group, entries] of Object.entries(theme.colors)) {
    for (const [key, value] of Object.entries(entries)) {
      assert.match(value, /^#[0-9a-f]{6}$/, `${group}.${key} 必须是 6 位小写 hex`)
      assert.ok(
        theme.palette.includes(value),
        `${group}.${key} 的 ${value} 不在 palette 中，请从标准色板取值`,
      )
    }
  }
})

test('chart theme exposes the keys the charts rely on', () => {
  for (const key of ['palette', 'colors', 'gridColor', 'axisColor', 'fontSize']) {
    assert.ok(key in theme, `chart-theme.json 缺少 ${key}`)
  }
  for (const metric of ['cost', 'duration', 'requests']) {
    assert.ok(metric in theme.colors.metric, `colors.metric 缺少 ${metric}`)
  }
  for (const p of ['p50', 'p95', 'avg']) {
    assert.ok(p in theme.colors.latency, `colors.latency 缺少 ${p}`)
  }
})
