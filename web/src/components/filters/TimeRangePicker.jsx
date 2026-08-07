// 全局时间范围选择：快捷项 + 自定义起止（含精确时间）+ 清除，对接 useTimeRange。

import React from 'react'
import { format } from 'date-fns'
import { cn } from '../../lib/utils'
import { Calendar } from '../ui/calendar'
import { Popover, PopoverContent, PopoverTrigger } from '../ui/popover'
import { quickRange } from '../../services/format'
import { useTimeRange } from '../../hooks/useTimeRange'

const PRESETS = [
  { key: 'today', label: '今天' },
  { key: 'yesterday', label: '昨天' },
  { key: '24h', label: '最近 24 小时' },
  { key: '7d', label: '最近 7 天' },
  { key: '30d', label: '最近 30 天' },
]

export default function TimeRangePicker({ className }) {
  const { range, setRange } = useTimeRange()
  const [open, setOpen] = React.useState(false)

  const fromDate = range?.from ? new Date(range.from) : undefined
  const toDate = range?.to ? new Date(range.to) : undefined

  const applyPreset = (key) => {
    const r = quickRange(key)
    setRange(r)
    setOpen(false)
  }

  const onSelect = (sel) => {
    // 区间选择：起始取当天 00:00，结束取当天 23:59:59，保留精确时间
    if (sel?.from && sel?.to) {
      const from = new Date(sel.from)
      from.setHours(0, 0, 0, 0)
      const to = new Date(sel.to)
      to.setHours(23, 59, 59, 999)
      setRange({ from: from.toISOString(), to: to.toISOString() })
      setOpen(false)
    }
  }

  const clear = () => {
    // 清除自定义选择，回退到默认最近 7 天
    setRange(quickRange('7d'))
    setOpen(false)
  }

  const label = fromDate && toDate
    ? `${format(fromDate, 'MM/dd HH:mm')} ~ ${format(toDate, 'MM/dd HH:mm')}`
    : '选择时间范围'

  return (
    <div className={cn('grid gap-2', className)}>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            type="button"
            className="btn px-2.5 min-w-[16rem] bg-white border-gray-200 hover:border-gray-300 dark:border-gray-700/60 dark:hover:border-gray-600 dark:bg-gray-800 text-gray-600 hover:text-gray-800 dark:text-gray-300 dark:hover:text-gray-100 font-medium text-left justify-start"
          >
            <svg className="fill-current text-gray-400 dark:text-gray-500 ml-1 mr-2" width="16" height="16" viewBox="0 0 16 16">
              <path d="M5 4a1 1 0 0 0 0 2h6a1 1 0 1 0 0-2H5Z" />
              <path d="M4 0a4 4 0 0 0-4 4v8a4 4 0 0 0 4 4h8a4 4 0 0 0 4-4V4a4 4 0 0 0-4-4H4ZM2 4a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V4Z" />
            </svg>
            <span className="tabular-nums">{label}</span>
            <svg className="fill-current text-gray-400 dark:text-gray-500 ml-auto mr-1" width="12" height="12" viewBox="0 0 12 12">
              <path d="M6 8.8 1.2 4h9.6L6 8.8Z" />
            </svg>
          </button>
        </PopoverTrigger>
        <PopoverContent className="w-auto p-3" align="end">
          <div className="flex flex-col sm:flex-row gap-4">
            {/* 快捷范围列（左侧） */}
            <div className="flex flex-col gap-0.5 w-32 shrink-0">
              <div className="text-xs font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide px-2 pb-1">快捷范围</div>
              {PRESETS.map((p) => (
                <button
                  key={p.key}
                  type="button"
                  onClick={() => applyPreset(p.key)}
                  className="text-left text-sm text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700/40 rounded-lg px-2 py-1.5"
                >
                  {p.label}
                </button>
              ))}
            </div>
            {/* 日历（右侧，箭头约束在自身边界） */}
            <div className="relative">
              <Calendar mode="range" defaultMonth={fromDate} selected={fromDate && toDate ? { from: fromDate, to: toDate } : undefined} onSelect={onSelect} />
            </div>
          </div>
          <div className="flex items-center justify-between border-t border-gray-100 dark:border-gray-700/60 mt-3 pt-3">
            {fromDate && toDate ? (
              <span className="text-xs text-gray-400 dark:text-gray-500 tabular-nums">
                {format(fromDate, 'MM/dd HH:mm')} ~ {format(toDate, 'MM/dd HH:mm')}
              </span>
            ) : (
              <span className="text-xs text-gray-400 dark:text-gray-500">选择起止日期</span>
            )}
            <button
              type="button"
              onClick={clear}
              className="text-xs text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200 border border-gray-200 dark:border-gray-600 rounded-lg px-2.5 py-1.5"
            >
              清除选择
            </button>
          </div>
        </PopoverContent>
      </Popover>
    </div>
  )
}
