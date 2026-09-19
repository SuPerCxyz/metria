import React, { useMemo } from 'react'
import { sortItems } from '../../services/sorting.js'

/// 业务列表：按首个选项字段降序取前 N 项（不提供交互排序控件）。
export default function SortableList({ items, options, limit, renderItem, empty, className = '' }) {
  const activeOption = options[0]
  const rows = useMemo(() => {
    const sorted = sortItems(items, activeOption?.getValue || (() => undefined), -1)
    return limit == null ? sorted : sorted.slice(0, limit)
  }, [activeOption, items, limit])

  if (rows.length === 0) {
    return empty || <div className="py-8 text-center text-sm text-gray-400 dark:text-gray-500">暂无数据</div>
  }
  return <div className={className}>{rows.map((item, index) => renderItem(item, index))}</div>
}
