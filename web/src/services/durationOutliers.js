// 调用时长口径告警的渲染判定与展示辅助。
//
// 后端 GET /api/v1/data-quality 的 duration_outliers 描述的是「疑似采集口径异常」
// （turn 起点被用作调用起点 / 同回合重复累计），不等同于真实模型响应时长。
// 旧后端或未启用时该字段为 null / 缺失，页面不渲染该区块。

/**
 * 只保留 count > 0 的检查项。
 * 0 条不是告警：把它渲染出来会制造「有问题」的假信号。
 */
export function activeChecks(checks) {
  if (!Array.isArray(checks)) return []
  return checks.filter((check) => check && Number(check.count) > 0)
}

/**
 * duration_outliers 是否可渲染。
 * 字段缺失 / 非对象 / checks 为空或全为 0 → false（不渲染，也允许页面的空态正常出现）。
 */
export function hasDurationOutliers(payload) {
  if (!payload || typeof payload !== 'object') return false
  if (!Array.isArray(payload.checks) || payload.checks.length === 0) return false
  return activeChecks(payload.checks).length > 0
}

/**
 * 取告警区块的整体严重级：任一 error 即 error，否则有 warning 即 warning，无则 null。
 * 仅在 hasDurationOutliers 为 true 后调用。
 */
export function topSeverity(checks) {
  const list = activeChecks(checks)
  if (list.some((check) => String(check.severity) === 'error')) return 'error'
  if (list.some((check) => String(check.severity) === 'warning')) return 'warning'
  return null
}

/** 小时数：101925.1 → "101,925.1 小时"；缺失或非数值返回 "—"。 */
export function fmtHours(hours) {
  if (hours === null || hours === undefined) return '—'
  const value = Number(hours)
  if (!Number.isFinite(value)) return '—'
  return `${value.toLocaleString('en-US', { maximumFractionDigits: 1 })} 小时`
}
