// 排行数据：按展示值降序，数值相同时保持原顺序。

export function sortRankingItems(items, valueKey, limit) {
  return (items || [])
    .map((item, index) => {
      const value = Number(item?.[valueKey] ?? 0)
      return { item, index, value: Number.isFinite(value) ? value : 0 }
    })
    .sort((a, b) => b.value - a.value || a.index - b.index)
    .slice(0, limit)
    .map(({ item }) => item)
}
