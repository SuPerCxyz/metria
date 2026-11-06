import { sumTokens } from '../../services/format.js'

/** 会话趋势数据：延迟必须使用单次调用平均值，不能把每小时总时长当作延迟。 */
export function buildSessionTrend(points, tab) {
  const series = Array.isArray(points) ? points : []
  switch (tab) {
    case 'tokens':
      return { labels: series.map((p) => p.bucket), values: series.map((p) => sumTokens(p)) }
    case 'cost':
      return { labels: series.map((p) => p.bucket), values: series.map((p) => p.cost_micro_usd ?? null) }
    case 'latency':
      return { labels: series.map((p) => p.bucket), values: series.map((p) => p.avg_duration_ms ?? null) }
    default:
      return { labels: [], values: [] }
  }
}
