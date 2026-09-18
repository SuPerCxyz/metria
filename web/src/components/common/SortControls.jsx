import React, { useId } from 'react'

export default function SortControls({ id, options, value, direction, onValueChange, onDirectionToggle }) {
  const generatedId = useId()
  if (!options || options.length <= 1) return null
  const controlId = id || `sort-${generatedId}`
  return (
    <div className="flex items-center justify-end gap-2">
      <label htmlFor={controlId} className="sr-only">排序字段</label>
      <select
        id={controlId}
        value={value}
        onChange={(event) => onValueChange(event.target.value)}
        className="rounded border border-gray-200 bg-white px-2 py-1 text-xs text-gray-500 focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500/30 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-300"
      >
        {options.map((option) => <option key={option.key} value={option.key}>{option.label}</option>)}
      </select>
      <button
        type="button"
        onClick={onDirectionToggle}
        aria-label={`切换为${direction === -1 ? '升序' : '降序'}`}
        className="rounded border border-gray-200 px-2 py-1 text-xs text-gray-500 hover:bg-gray-50 focus-visible:outline-2 focus-visible:outline-indigo-500 dark:border-gray-700 dark:text-gray-300 dark:hover:bg-gray-700/40"
      >
        {direction === -1 ? '降序' : '升序'}
      </button>
    </div>
  )
}
