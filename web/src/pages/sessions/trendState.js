/** 趋势卡片的渲染状态：加载/错误优先于空数据，避免把请求失败显示成「暂无调用」。 */
export function trendViewState({ loading, error, labels }) {
  if (loading) return 'loading'
  if (error) return 'error'
  if (!Array.isArray(labels) || labels.length === 0) return 'empty'
  return 'chart'
}
