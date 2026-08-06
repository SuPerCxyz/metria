// 详情摘要：KV 网格，用于详情页顶部摘要。

import React from 'react'

export default function DetailSummary({ items }) {
  return (
    <div className="grid grid-cols-2 sm:grid-cols-3 xl:grid-cols-4 gap-4">
      {items.map((it, i) => (
        <div key={i} className="bg-gray-50 dark:bg-gray-700/30 rounded-xl p-3">
          <div className="text-xs text-gray-400 dark:text-gray-500">{it.label}</div>
          <div className="mt-0.5 text-sm font-semibold text-gray-800 dark:text-gray-100 tabular-nums">{it.value}</div>
        </div>
      ))}
    </div>
  )
}
