// 排行列表：排名 + 名称 + 数值条。默认前 N 项。

import React, { useMemo, useState } from 'react'
import { sortRankingItems } from '../../services/ranking'
import SortControls from '../common/SortControls'

export default function RankingList({ items, valueKey, labelKey, format, secondaryKey, secondaryLabel = '费用', secondaryFormat, limit = 5, onItemClick }) {
  const sortOptions = useMemo(() => [
    { key: valueKey, label: '数值' },
    ...(secondaryKey ? [{ key: secondaryKey, label: secondaryLabel }] : []),
    { key: labelKey || 'name', label: '名称' },
  ].filter((option, index, options) => options.findIndex((item) => item.key === option.key) === index), [labelKey, secondaryKey, secondaryLabel, valueKey])
  const [sortKey, setSortKey] = useState(valueKey)
  const [sortDir, setSortDir] = useState(-1)
  const rows = sortRankingItems(items, valueKey, limit, sortKey, sortDir)
  const max = Math.max(1, ...rows.map((r) => Number(r[valueKey] ?? 0)))

  return (
    <div>
      {sortOptions.length > 1 && (
        <div className="mb-2 flex items-center justify-end gap-2">
          <SortControls
            options={sortOptions}
            value={sortKey}
            direction={sortDir}
            onValueChange={(key) => {
              setSortKey(key)
              setSortDir(-1)
            }}
            onDirectionToggle={() => setSortDir((direction) => direction === -1 ? 1 : -1)}
          />
        </div>
      )}
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
              <span className="flex items-baseline justify-between gap-3">
                <span className="min-w-0 truncate text-sm font-medium text-gray-700 dark:text-gray-200">{label}</span>
                <span className="shrink-0 text-sm font-semibold text-gray-800 dark:text-gray-100 tabular-nums">{format ? format(v) : v.toLocaleString()}</span>
              </span>
              {secondaryKey && (
                <span className="block mt-0.5 text-xs text-gray-400 dark:text-gray-500 tabular-nums">
                  {secondaryLabel} {secondaryFormat ? secondaryFormat(item[secondaryKey]) : (item[secondaryKey] ?? '—')}
                </span>
              )}
              <span className="block h-1.5 mt-1 bg-gray-100 dark:bg-gray-700/40 rounded-full overflow-hidden">
                <span className="block h-full bg-indigo-500/70 dark:bg-indigo-400/70 rounded-full" style={{ width: `${pct}%` }} />
              </span>
            </span>
          </button>
        )
      })}
      </div>
    </div>
  )
}
