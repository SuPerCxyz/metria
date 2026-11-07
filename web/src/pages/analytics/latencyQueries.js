/** Analytics 延迟摘要/趋势的请求门控；详情标签需要摘要卡，但不请求趋势。 */
export function latencySummaryEnabled(tab) {
  return tab === 'latency' || tab === 'agent-detail' || tab === 'model-detail'
}

export function latencySeriesEnabled(tab) {
  return tab === 'latency'
}
