// 指标卡片：主数值 + 趋势值 + 辅助说明。桌面每行最多 4 张。

import React from 'react'

export default function MetricCard({ label, value, delta, deltaTone, sub, hint }) {
  return (
    <div className="flex flex-col col-span-full sm:col-span-6 xl:col-span-3 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-5">
      <div className="flex items-center justify-between mb-2">
        <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400">{label}</h3>
        {delta !== undefined && delta !== null && (
          <span
            className={`inline-flex items-center text-xs font-medium px-2 py-0.5 rounded-full ${
              deltaTone === 'up'
                ? 'text-emerald-600 dark:text-emerald-400 bg-emerald-100 dark:bg-emerald-400/10'
                : deltaTone === 'down'
                ? 'text-red-600 dark:text-red-400 bg-red-100 dark:bg-red-400/10'
                : 'text-gray-500 dark:text-gray-400 bg-gray-100 dark:bg-gray-700/40'
            }`}
          >
            {delta}
          </span>
        )}
      </div>
      <div className="text-2xl font-bold text-gray-800 dark:text-gray-100 tabular-nums tracking-tight">
        {value}
      </div>
      {sub && <div className="mt-1 text-xs text-gray-400 dark:text-gray-500">{sub}</div>}
      {hint && <div className="mt-1 text-[11px] text-gray-300 dark:text-gray-600">{hint}</div>}
    </div>
  )
}
