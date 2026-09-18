import React, { useMemo, useState } from 'react'
import { sortItems } from '../../services/sorting.js'
import SortControls from './SortControls'

export default function SortableList({ items, options, limit, renderItem, empty, className = '' }) {
  const [sortKey, setSortKey] = useState(options[0]?.key)
  const [sortDir, setSortDir] = useState(-1)
  const activeOption = options.find((option) => option.key === sortKey) || options[0]
  const rows = useMemo(() => {
    const sorted = sortItems(items, activeOption?.getValue || (() => undefined), sortDir)
    return limit == null ? sorted : sorted.slice(0, limit)
  }, [activeOption, items, limit, sortDir])

  return (
    <div>
      <div className="mb-2">
        <SortControls
          options={options}
          value={activeOption?.key}
          direction={sortDir}
          onValueChange={(key) => {
            setSortKey(key)
            setSortDir(-1)
          }}
          onDirectionToggle={() => setSortDir((direction) => direction === -1 ? 1 : -1)}
        />
      </div>
      {rows.length === 0 ? (empty || <div className="py-8 text-center text-sm text-gray-400 dark:text-gray-500">暂无数据</div>) : (
        <div className={className}>
          {rows.map((item, index) => renderItem(item, index))}
        </div>
      )}
    </div>
  )
}
