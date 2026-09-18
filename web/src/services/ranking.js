// 排行数据：按展示值降序，数值相同时保持原顺序。

import { sortItems } from './sorting.js'

export function sortRankingItems(items, valueKey, limit, sortKey = valueKey, direction = -1) {
  return sortItems(items, (item) => item?.[sortKey], direction).slice(0, limit)
}
