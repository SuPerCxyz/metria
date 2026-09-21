// 全局用量筛选：Agent、模型、节点。状态由 NodeFilterProvider 跨页面保持。
// Agent 与模型候选只包含当前时间范围内有数据的项，节点始终列出全部已登记节点。

import React, { useEffect, useMemo, useRef, useState } from 'react'
import { useNodeFilter } from '../../hooks/useNodeFilter'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { api, q, rangeParams } from '../../services/api'

const EMPTY = []

function withSelected(items, selected) {
  if (!selected || items.some((item) => item.id === selected)) return items
  return [{ id: selected, label: selected }, ...items]
}

function SelectField({ label, value, onChange, options, disabled }) {
  return (
    <label className="block">
      <span className="mb-1 block text-xs font-medium text-gray-400 dark:text-gray-500">{label}</span>
      <select
        value={value || ''}
        onChange={(event) => onChange(event.target.value || null)}
        disabled={disabled}
        className="w-full appearance-none rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm text-gray-700 focus:border-indigo-500 focus:outline-none focus:ring-2 focus:ring-indigo-500/30 dark:border-gray-600 dark:bg-gray-700/60 dark:text-gray-200"
      >
        <option value="">全部{label}</option>
        {options.map((item) => <option key={item.id} value={item.id}>{item.label || item.id}</option>)}
      </select>
    </label>
  )
}

export default function UsageFilterPicker({ className = '' }) {
  const [open, setOpen] = useState(false)
  const boxRef = useRef(null)

  // 点击页面其他区域或按 Esc 自动收起，无需额外确认
  useEffect(() => {
    if (!open) return undefined
    const onPointerDown = (event) => {
      if (boxRef.current && !boxRef.current.contains(event.target)) setOpen(false)
    }
    const onKeyDown = (event) => {
      if (event.key === 'Escape') setOpen(false)
    }
    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [open])
  const {
    nodeId, clientId, model,
    setNodeId, setClientId, setModel, clearFilters,
  } = useNodeFilter()
  const { range } = useTimeRange()
  // 候选项跟随全局时间范围刷新：Agent/模型只列范围内有数据的，节点全列。
  const filterRange = q(rangeParams(range))
  const options = useQuery(`usage-filter-options${filterRange}`, () => api(`/usage/filter-options${filterRange}`))
  const nodes = useMemo(() => withSelected(options.data?.nodes || EMPTY, nodeId), [options.data, nodeId])
  const agents = useMemo(() => withSelected(options.data?.agents || EMPTY, clientId), [options.data, clientId])
  const models = useMemo(() => withSelected(options.data?.models || EMPTY, model), [options.data, model])
  const active = [nodeId, clientId, model].filter(Boolean).length

  return (
    <div ref={boxRef} className={`relative shrink-0 ${className}`}>
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        aria-label="打开全局用量筛选"
        className="btn border-gray-200 bg-white px-3 text-sm font-medium text-gray-600 hover:border-gray-300 hover:text-gray-800 dark:border-gray-700/60 dark:bg-gray-800 dark:text-gray-300 dark:hover:border-gray-600 dark:hover:text-gray-100"
      >
        <svg className="mr-1.5 h-4 w-4 text-gray-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M4 6h16M7 12h10m-7 6h4" />
        </svg>
        筛选{active > 0 ? ` · ${active}` : ''}
      </button>
      {open && (
        <div className="absolute right-0 z-40 mt-2 w-72 rounded-xl border border-gray-200 bg-white p-3 shadow-xl dark:border-gray-700 dark:bg-gray-800">
          <div className="mb-3 flex items-center justify-between">
            <span className="text-sm font-semibold text-gray-700 dark:text-gray-200">全局筛选</span>
            <button type="button" onClick={clearFilters} className="text-xs text-indigo-600 hover:text-indigo-700 dark:text-indigo-400">清除</button>
          </div>
          <div className="space-y-2.5">
            <SelectField label="节点" value={nodeId} onChange={setNodeId} options={nodes} disabled={options.loading && nodes.length === 0} />
            <SelectField label="Agent" value={clientId} onChange={setClientId} options={agents} disabled={options.loading && agents.length === 0} />
            <SelectField label="模型" value={model} onChange={setModel} options={models} disabled={options.loading && models.length === 0} />
          </div>
          {options.error && <p className="mt-2 text-xs text-amber-600 dark:text-amber-400">筛选选项加载失败，可稍后刷新。</p>}
        </div>
      )}
    </div>
  )
}
