// 指标卡片：主数值 + 趋势值 + 辅助说明。桌面每行最多 4 张（span 可调）。

import React from 'react'

export default function MetricCard({ label, value, delta, deltaTone, sub, hint, span }) {
  const spanClass = span || 'xl:col-span-3'
  return (
    <div
      title={typeof hint === 'string' ? hint : undefined}
      className={`min-w-0 flex flex-col col-span-full sm:col-span-6 ${spanClass} bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-4`}
    >
      <div className="flex min-w-0 items-center justify-between gap-2 mb-1">
        <h3 className="min-w-0 truncate text-sm font-medium text-gray-500 dark:text-gray-400">{label}</h3>
        {delta !== undefined && delta !== null && (
          <span
            className={`inline-flex shrink-0 items-center text-xs font-medium px-2 py-0.5 rounded-full ${
              deltaTone === 'up'
                ? 'text-white bg-emerald-500'
                : deltaTone === 'down'
                ? 'text-white bg-red-500'
                : 'text-white bg-gray-500'
            }`}
          >
            {delta}
          </span>
        )}
      </div>
      <div className="min-w-0 text-2xl font-bold text-gray-800 dark:text-gray-100 tabular-nums tracking-tight">
        {value}
      </div>
      {sub && (
        <div title={typeof sub === 'string' ? sub : undefined} className="mt-1 min-w-0 truncate whitespace-nowrap text-xs text-gray-400 dark:text-gray-500">
          {sub}
        </div>
      )}
    </div>
  )
}
