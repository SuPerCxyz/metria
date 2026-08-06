// 全局时间范围选择：快捷项 + 自定义起止，对接 useTimeRange。

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
    if (sel?.from && sel?.to) {
      setRange({ from: sel.from.toISOString(), to: sel.to.toISOString() })
      setOpen(false)
    }
  }

  const label = fromDate && toDate
    ? `${format(fromDate, 'LLL dd')} - ${format(toDate, 'LLL dd, y')}`
    : '选择时间范围'

  return (
    <div className={cn('grid gap-2', className)}>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            type="button"
            className="btn px-2.5 min-w-[15rem] bg-white border-gray-200 hover:border-gray-300 dark:border-gray-700/60 dark:hover:border-gray-600 dark:bg-gray-800 text-gray-600 hover:text-gray-800 dark:text-gray-300 dark:hover:text-gray-100 font-medium text-left justify-start"
          >
            <svg className="fill-current text-gray-400 dark:text-gray-500 ml-1 mr-2" width="16" height="16" viewBox="0 0 16 16">
              <path d="M5 4a1 1 0 0 0 0 2h6a1 1 0 1 0 0-2H5Z" />
              <path d="M4 0a4 4 0 0 0-4 4v8a4 4 0 0 0 4 4h8a4 4 0 0 0 4-4V4a4 4 0 0 0-4-4H4ZM2 4a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V4Z" />
            </svg>
            {label}
            <svg className="fill-current text-gray-400 dark:text-gray-500 ml-auto mr-1" width="12" height="12" viewBox="0 0 12 12">
              <path d="M6 8.8 1.2 4h9.6L6 8.8Z" />
            </svg>
          </button>
        </PopoverTrigger>
        <PopoverContent className="w-auto p-3" align="end">
          <div className="flex flex-col sm:flex-row gap-3">
            <div className="flex flex-col gap-1 w-32">
              <div className="text-xs font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide px-1 pb-1">快捷范围</div>
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
            <Calendar mode="range" defaultMonth={fromDate} selected={fromDate && toDate ? { from: fromDate, to: toDate } : undefined} onSelect={onSelect} />
          </div>
        </PopoverContent>
      </Popover>
    </div>
  )
}
