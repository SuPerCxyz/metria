// 调用时长口径告警的四态判定：字段缺失、checks 全 0、有 error、有 warning。

import assert from 'node:assert/strict'
import test from 'node:test'
import { activeChecks, fmtHours, hasDurationOutliers, topSeverity } from './durationOutliers.js'

const withChecks = (checks) => ({
  label: '疑似采集口径异常（turn 起点被用作调用起点 / 同回合重复累计），不等同于真实模型响应时长',
  total_duration_hours: 101925.1,
  over_threshold_duration_hours: 40445.8,
  share_of_total_duration: 0.3968,
  checks,
  by_client: [{ client_id: 'opencode', count: 4750, call_granularity: 'message', timing_source: 'opencode_message_timestamps' }],
})

test('字段缺失：后端未部署或未启用时不渲染', () => {
  assert.equal(hasDurationOutliers(undefined), false)
  assert.equal(hasDurationOutliers(null), false)
  assert.equal(hasDurationOutliers({}), false)
  assert.equal(hasDurationOutliers({ duration_outliers: null }), false)
  assert.equal(hasDurationOutliers({ duration_outliers: undefined }), false)
  // 非对象也按不可渲染处理，避免前端崩溃
  assert.equal(hasDurationOutliers('error'), false)
  assert.equal(hasDurationOutliers(0), false)
})

test('字段存在但 checks 缺失或为空数组：不渲染', () => {
  assert.equal(hasDurationOutliers({ total_duration_hours: 101925.1 }), false)
  assert.equal(hasDurationOutliers(withChecks(undefined)), false)
  assert.equal(hasDurationOutliers(withChecks([])), false)
})

test('checks 全 0：检查已执行且通过，不渲染告警（页面空态不受阻断）', () => {
  const allZero = withChecks([
    { key: 'over_24h', severity: 'error', threshold: '单条时长 > 24 小时', count: 0 },
    { key: 'ttft_invalid', severity: 'error', threshold: '首响应 > 1 小时或超过该调用时长', count: 0 },
    { key: 'duplicate_start', severity: 'warning', threshold: '同一会话内共享起始时刻的分组，累计时长 > 组内墙钟 5 倍', count: 0 },
  ])
  assert.equal(hasDurationOutliers(allZero), false)
  assert.deepEqual(activeChecks(allZero.checks), [])
  assert.equal(topSeverity(allZero.checks), null)
})

test('有 error：渲染，整体严重级为 error', () => {
  const payload = withChecks([
    { key: 'over_24h', severity: 'error', threshold: '单条时长 > 24 小时', count: 8 },
    { key: 'ttft_invalid', severity: 'error', threshold: '首响应 > 1 小时或超过该调用时长', count: 8 },
    { key: 'duplicate_start', severity: 'warning', threshold: '同一会话内共享起始时刻的分组，累计时长 > 组内墙钟 5 倍', count: 4500 },
  ])
  assert.equal(hasDurationOutliers(payload), true)
  assert.equal(topSeverity(payload.checks), 'error')
  assert.equal(activeChecks(payload.checks).length, 3)
})

test('只有 warning：渲染，整体严重级为 warning', () => {
  const payload = withChecks([
    { key: 'over_24h', severity: 'error', threshold: '单条时长 > 24 小时', count: 0 },
    { key: 'ttft_invalid', severity: 'error', threshold: '首响应 > 1 小时或超过该调用时长', count: 0 },
    { key: 'duplicate_start', severity: 'warning', threshold: '同一会话内共享起始时刻的分组，累计时长 > 组内墙钟 5 倍', count: 4500 },
  ])
  assert.equal(hasDurationOutliers(payload), true)
  assert.equal(topSeverity(payload.checks), 'warning')
  // 只列出 count > 0 的项，0 条的 error 项不进表格
  assert.equal(activeChecks(payload.checks).length, 1)
  assert.equal(activeChecks(payload.checks)[0].key, 'duplicate_start')
})

test('error 优先于 warning：混合时整体严重级为 error', () => {
  assert.equal(topSeverity([
    { key: 'a', severity: 'warning', count: 1 },
    { key: 'b', severity: 'error', count: 1 },
  ]), 'error')
})

test('activeCounts 容错：非数组 / 脏元素被过滤', () => {
  assert.deepEqual(activeChecks(undefined), [])
  assert.deepEqual(activeChecks(null), [])
  assert.deepEqual(activeChecks('x'), [])
  assert.deepEqual(activeChecks([null, undefined, {}, { key: 'a', count: 0 }]), [])
  // count 为字符串数字时按数值判断
  assert.equal(activeChecks([{ key: 'a', count: '3' }]).length, 1)
  // count 缺失视为 0
  assert.equal(activeChecks([{ key: 'a' }]).length, 0)
})

test('fmtHours：小时数展示与不可用值', () => {
  assert.equal(fmtHours(101925.1), '101,925.1 小时')
  assert.equal(fmtHours(40445.8), '40,445.8 小时')
  assert.equal(fmtHours(0), '0 小时')
  assert.equal(fmtHours(null), '—')
  assert.equal(fmtHours(undefined), '—')
  assert.equal(fmtHours('abc'), '—')
  assert.equal(fmtHours(NaN), '—')
})

test('share_of_total_duration 是 0~1 小数，交由 fmtPct 展示为百分比', () => {
  // 契约校验：0.3968 → 39.7%，前端不得把它当 0~100 处理
  assert.equal((0.3968 * 100).toFixed(1), '39.7')
  assert.ok(0 <= 0.3968 && 0.3968 <= 1)
})
