// 排行列表：排名 + 名称 + 数值条。默认前 N 项。

import React from 'react'

export default function RankingList({ items, valueKey, labelKey, format, limit = 5, onItemClick }) {
  const rows = (items || []).slice(0, limit)
  const max = Math.max(1, ...rows.map((r) => Number(r[valueKey] ?? 0)))

  return (
    <div className="space-y-1">
      {rows.length === 0 && <div className="text-sm text-gray-400 dark:text-gray-500 py-8 text-center">暂无数据</div>}
      {rows.map((item, i) => {
        const v = Number(item[valueKey] ?? 0)
        const pct = (v / max) * 100
        const label = labelKey ? item[labelKey] : item.name || item.label || item.id
        return (
          <button
            key={i}
            type="button"
            onClick={() => onItemClick?.(item)}
            className={`w-full flex items-center gap-3 px-3 py-2 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700/30 text-left ${onItemClick ? 'cursor-pointer' : ''}`}
          >
            <span className="w-5 text-sm font-semibold text-gray-400 dark:text-gray-500 tabular-nums">{i + 1}</span>
            <span className="flex-1 min-w-0">
              <span className="block text-sm font-medium text-gray-700 dark:text-gray-200 truncate">{label}</span>
              <span className="block h-1.5 mt-1 bg-gray-100 dark:bg-gray-700/40 rounded-full overflow-hidden">
                <span className="block h-full bg-indigo-500/70 dark:bg-indigo-400/70 rounded-full" style={{ width: `${pct}%` }} />
              </span>
            </span>
            <span className="text-sm font-semibold text-gray-800 dark:text-gray-100 tabular-nums">{format ? format(v) : v.toLocaleString()}</span>
          </button>
        )
      })}
    </div>
  )
}
