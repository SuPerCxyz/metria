// 归档水位相关的展示判定：明细列表的范围关系 + 活动类字段的不可计算标注。
// 核心原则：被归档掩掉的数字绝不能渲染成 0、— 或「暂无数据」。

/** 归档命中时的统一展示文案：不可计算 ≠ 无数据。 */
export const ARCHIVED_UNAVAILABLE_LABEL = '已归档，不可计算'

/**
 * 判断所选范围与归档水位（archived_before）的关系。
 * 返回 { level, watermark }：
 * - none：无水位、字段缺失或无法解析，完全不影响现有行为；
 * - partial：范围起点早于水位、终点晚于水位（部分时段已归档）；
 * - full：整段范围都在水位之前（全部已归档）。
 */
export function archiveRangeState(archivedBefore, from, to) {
  if (!archivedBefore) return { level: 'none', watermark: null }
  const watermark = new Date(archivedBefore)
  const start = new Date(from || '')
  if (Number.isNaN(watermark.getTime()) || Number.isNaN(start.getTime())) {
    return { level: 'none', watermark: null }
  }
  if (start >= watermark) return { level: 'none', watermark }
  // 起点已确定早于水位：终点解析失败时也至少是部分归档，不隐瞒水位
  const end = new Date(to || '')
  if (Number.isNaN(end.getTime())) return { level: 'partial', watermark }
  return { level: end <= watermark ? 'full' : 'partial', watermark }
}

/** 列表为空时的空态文案：命中归档时返回专用文案，否则返回 undefined 走默认空态。 */
export function archiveEmptyText(state) {
  if (state?.level === 'full') return '所选时段明细已归档，不可查看'
  if (state?.level === 'partial') return '水位之前的明细已归档，可查时段暂无数据'
  return undefined
}

/**
 * 活动类字段的归档判定（/overview 的 message_count、user_message_count、
 * tool_call_count、active_sessions、session_duration_ms、sessions 同一规则）：
 * - activity_archived_before 为 null/缺失（旧后端）→ 恒为 false，渲染路径与改动前完全一致；
 * - 水位非 null 且字段被后端置 null → true，调用方显示「已归档，不可计算」（不是无数据）；
 * - 水位非 null 但字段有值 → false，按真实值正常展示。
 */
export function isActivityArchived(activityArchivedBefore, value) {
  if (!activityArchivedBefore) return false
  return value === null || value === undefined
}
