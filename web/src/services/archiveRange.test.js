// 归档水位与时间范围的关系判定：字段缺失不影响现有行为，命中时必须诚实标注。

import assert from 'node:assert/strict'
import test from 'node:test'
import { archiveEmptyText, archiveRangeState, ARCHIVED_UNAVAILABLE_LABEL, isActivityArchived } from './archiveRange.js'

const WATERMARK = '2026-08-23T00:00:00Z'

test('水位字段缺失或无法解析时按 none 处理，不影响现有行为', () => {
  assert.equal(archiveRangeState(null, '2026-07-01T00:00:00Z', '2026-09-01T00:00:00Z').level, 'none')
  assert.equal(archiveRangeState(undefined, '2026-07-01T00:00:00Z', '2026-09-01T00:00:00Z').level, 'none')
  assert.equal(archiveRangeState('', '2026-07-01T00:00:00Z', '2026-09-01T00:00:00Z').level, 'none')
  assert.equal(archiveRangeState('not-a-date', '2026-07-01T00:00:00Z', '2026-09-01T00:00:00Z').level, 'none')
  assert.equal(archiveRangeState(WATERMARK, 'not-a-date', '2026-09-01T00:00:00Z').level, 'none')
})

test('范围起点不早于水位时不提示', () => {
  assert.equal(archiveRangeState(WATERMARK, '2026-09-01T00:00:00Z', '2026-10-01T00:00:00Z').level, 'none')
  // 起点恰好等于水位：该点之后的明细都可查
  assert.equal(archiveRangeState(WATERMARK, WATERMARK, '2026-10-01T00:00:00Z').level, 'none')
})

test('整段早于水位为 full，终点恰好等于水位也算 full', () => {
  assert.equal(archiveRangeState(WATERMARK, '2026-07-01T00:00:00Z', '2026-08-01T00:00:00Z').level, 'full')
  assert.equal(archiveRangeState(WATERMARK, '2026-07-01T00:00:00Z', WATERMARK).level, 'full')
})

test('跨水位为 partial', () => {
  assert.equal(archiveRangeState(WATERMARK, '2026-08-01T00:00:00Z', '2026-09-01T00:00:00Z').level, 'partial')
})

test('起点早于水位、终点无法解析时按 partial 提示，不隐瞒水位', () => {
  assert.equal(archiveRangeState(WATERMARK, '2026-08-01T00:00:00Z', 'bad').level, 'partial')
})

test('返回的水位是可格式化的日期', () => {
  const state = archiveRangeState(WATERMARK, '2026-07-01T00:00:00Z', '2026-09-01T00:00:00Z')
  assert.equal(state.watermark.toISOString(), '2026-08-23T00:00:00.000Z')
})

test('命中归档时空态文案不是「暂无数据」，未命中返回 undefined', () => {
  const full = archiveRangeState(WATERMARK, '2026-07-01T00:00:00Z', '2026-08-01T00:00:00Z')
  const partial = archiveRangeState(WATERMARK, '2026-08-01T00:00:00Z', '2026-09-01T00:00:00Z')
  const none = archiveRangeState(null, '2026-07-01T00:00:00Z', '2026-09-01T00:00:00Z')
  assert.match(archiveEmptyText(full), /已归档/)
  assert.match(archiveEmptyText(partial), /已归档/)
  assert.equal(archiveEmptyText(none), undefined)
  assert.equal(archiveEmptyText(undefined), undefined)
})

// —— 活动类字段判定（activity_archived_before × 字段值 四种组合）——

test('组合一：水位为 null 时一切字段走原渲染路径（旧后端字段缺失同样处理）', () => {
  assert.equal(isActivityArchived(null, null), false)
  assert.equal(isActivityArchived(null, 0), false)
  assert.equal(isActivityArchived(null, 337), false)
  // 响应缺少 activity_archived_before 字段（undefined）也不得改变渲染
  assert.equal(isActivityArchived(undefined, null), false)
  assert.equal(isActivityArchived('', 42), false)
})

test('组合二：水位非 null 且字段为 null → 归档不可计算（含 sessions）', () => {
  const messageCount = null
  const sessions = null // 带 node/client/model 筛选时后端将 sessions 置 null
  assert.equal(isActivityArchived(WATERMARK, messageCount), true)
  assert.equal(isActivityArchived(WATERMARK, sessions), true, 'sessions 必须与 message_count 同一规则')
  assert.equal(isActivityArchived(WATERMARK, undefined), true)
  // 命中时使用统一文案，绝不显示 0/—/暂无数据
  assert.equal(ARCHIVED_UNAVAILABLE_LABEL, '已归档，不可计算')
})

test('组合三：水位非 null 但字段有值 → 按真实值正常展示', () => {
  const sessions = 337 // 无筛选时 sessions 走聚合表，归档前后一致
  const messageCount = 42
  assert.equal(isActivityArchived(WATERMARK, sessions), false)
  assert.equal(isActivityArchived(WATERMARK, messageCount), false)
  assert.equal(isActivityArchived(WATERMARK, 0), false) // 真实的 0 也按原路径显示 0
})

test('组合四：水位为 null 且 session_duration_ms 为 null → 沿用原「无样本」展示，不误标成归档', () => {
  assert.equal(isActivityArchived(null, null), false)
  assert.equal(isActivityArchived(null, undefined), false)
})
