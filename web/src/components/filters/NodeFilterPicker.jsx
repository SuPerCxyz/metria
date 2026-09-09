// 全局节点过滤器：全部节点 / 单个节点，选中后统计页面查询自动跟随。

import React from 'react'
import { useNodeFilter } from '../../hooks/useNodeFilter'
import { useNodeNames } from '../../hooks/useNodeNames'

export default function NodeFilterPicker({ className = '' }) {
  const { nodeId, setNodeId } = useNodeFilter()
  const names = useNodeNames()
  const nodes = Object.entries(names).sort((a, b) => (a[1] || a[0]).localeCompare(b[1] || b[0]))

  return (
    <div className={`relative shrink-0 ${className}`} title="按节点过滤统计数据">
      <select
        value={nodeId || ''}
        onChange={(e) => setNodeId(e.target.value || null)}
        aria-label="按节点过滤"
        className="w-full sm:w-auto max-w-52 truncate appearance-none text-sm text-gray-600 dark:text-gray-300 bg-gray-100 dark:bg-gray-700/40 rounded-lg pl-3 pr-7 py-2 focus:outline-none focus:ring-2 focus:ring-indigo-500/40 cursor-pointer"
      >
        <option value="">全部节点</option>
        {nodes.map(([id, name]) => (
          <option key={id} value={id}>
            {name === id ? id : `${name}`}
          </option>
        ))}
      </select>
      <svg
        className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 w-3 h-3 text-gray-400 dark:text-gray-500"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d="m6 9 6 6 6-6" />
      </svg>
    </div>
  )
}
