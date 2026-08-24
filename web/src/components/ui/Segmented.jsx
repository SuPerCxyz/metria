// 分段选择控件：选中项用主色高亮，未选中灰字，对比明显。
// 替换各处手写的 px-3 py-1.5 rounded-md + bg-white 弱对比样式。

import React from 'react'
import { cn } from '../../lib/utils'

export default function Segmented({ items, value, onChange, className }) {
  return (
    <div className={cn('inline-flex rounded-lg bg-gray-100 dark:bg-gray-700/40 p-0.5', className)}>
      {items.map((item) => {
        const key = typeof item === 'object' ? item.key : item
        const label = typeof item === 'object' ? item.label : item
        const active = value === key
        return (
          <button
            key={key}
            type="button"
            onClick={() => onChange(key)}
            aria-pressed={active}
            className={cn(
              'px-3 py-1.5 text-sm font-medium rounded-md transition-colors',
              active
                ? 'bg-indigo-600 dark:bg-indigo-500 text-white shadow-sm'
                : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200'
            )}
          >
            {label}
          </button>
        )
      })}
    </div>
  )
}
