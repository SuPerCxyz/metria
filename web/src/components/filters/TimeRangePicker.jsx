// 全局时间范围选择：快捷项联动预览 + 日历选择需确认生效。

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
  // 日历临时选择（未确认），初始为当前范围
  const [draft, setDraft] = React.useState(null)

  const fromDate = range?.from ? new Date(range.from) : undefined
  const toDate = range?.to ? new Date(range.to) : undefined

  const openPicker = (o) => {
    setOpen(o)
    if (o) {
      // 打开时日历默认选中当前范围
      setDraft(fromDate && toDate ? { from: fromDate, to: toDate } : { from: new Date(), to: new Date() })
    }
  }

  // 快捷范围：hover 联动日历预览
  const previewPreset = (key) => {
    const r = quickRange(key)
    setDraft({ from: new Date(r.from), to: new Date(r.to) })
  }

  // 快捷范围：点击立即生效并关闭
  const applyPreset = (key) => {
    const r = quickRange(key)
    setRange(r)
    setOpen(false)
  }

  // 日历选择：第一击设起点（保留当前 to 作为临时终点），第二击定终点；均不立即生效
  const onSelect = (sel) => {
    if (!sel?.from) return
    const from = new Date(sel.from)
    from.setHours(0, 0, 0, 0)
    if (sel.to) {
      // 第二击：完成范围
      const to = new Date(sel.to)
      to.setHours(23, 59, 59, 999)
      setDraft({ from, to })
    } else {
      // 第一击：选起点，终点沿用之前范围的 to（若有），否则单日
      const prevTo = draft?.to
      if (prevTo) {
        const to = new Date(prevTo)
        setDraft({ from, to })
      } else {
        setDraft({ from, to: new Date(from) })
      }
    }
  }

  // 起始/结束时间调整（HH:mm），只改时分保留日期
  const setDraftTime = (which, timeStr) => {
    if (!draft) return
    const d = new Date(which === 'from' ? draft.from : draft.to)
    const [hh, mm] = timeStr.split(':').map(Number)
    if (Number.isFinite(hh) && Number.isFinite(mm)) {
      d.setHours(hh, mm, 0, 0)
      setDraft(which === 'from' ? { from: d, to: draft.to } : { from: draft.from, to: d })
    }
  }

  // 确认生效
  const confirm = () => {
    if (draft?.from && draft?.to) {
      setRange({ from: draft.from.toISOString(), to: draft.to.toISOString() })
    }
    setOpen(false)
  }

  // 取消：放弃临时选择，恢复当前范围
  const cancel = () => {
    setDraft(fromDate && toDate ? { from: fromDate, to: toDate } : null)
    setOpen(false)
  }

  // 清除：回退默认最近 7 天
  const clear = () => {
    setRange(quickRange('7d'))
    setOpen(false)
  }

  const label = fromDate && toDate
    ? `${format(fromDate, 'MM/dd HH:mm')} ~ ${format(toDate, 'MM/dd HH:mm')}`
    : '选择时间范围'

  const selected = draft ? { from: draft.from, to: draft.to } : undefined

  return (
    <div className={cn('grid gap-2', className)}>
      <Popover open={open} onOpenChange={openPicker}>
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
            {/* 快捷范围列（左侧，hover 联动日历） */}
            <div className="flex flex-col gap-0.5 w-32 shrink-0">
              <div className="text-xs font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wide px-2 pb-1">快捷范围</div>
              {PRESETS.map((p) => (
                <button
                  key={p.key}
                  type="button"
                  onMouseEnter={() => previewPreset(p.key)}
                  onClick={() => applyPreset(p.key)}
                  className="text-left text-sm text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700/40 rounded-lg px-2 py-1.5"
                >
                  {p.label}
                </button>
              ))}
            </div>
            {/* 日历（右侧，时间输入在日历下方） */}
            <div className="flex flex-col gap-3">
              <div className="relative">
                <Calendar mode="range" defaultMonth={draft?.from || fromDate} selected={selected} onSelect={onSelect} />
              </div>
              {/* 起始/结束时间输入（日历下方） */}
              {draft?.from && draft?.to ? (
                <div className="flex items-center justify-center gap-2 border-t border-gray-100 dark:border-gray-700/60 pt-3">
                  <span className="inline-flex items-center gap-1.5">
                    <span className="text-xs text-gray-400 dark:text-gray-500">起始</span>
                    <input
                      type="time"
                      value={format(draft.from, 'HH:mm')}
                      onChange={(e) => setDraftTime('from', e.target.value)}
                      className="text-xs px-1.5 py-1 rounded border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-800 text-gray-700 dark:text-gray-200"
                    />
                  </span>
                  <span className="inline-flex items-center gap-1.5">
                    <span className="text-xs text-gray-400 dark:text-gray-500">结束</span>
                    <input
                      type="time"
                      value={format(draft.to, 'HH:mm')}
                      onChange={(e) => setDraftTime('to', e.target.value)}
                      className="text-xs px-1.5 py-1 rounded border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-800 text-gray-700 dark:text-gray-200"
                    />
                  </span>
                </div>
              ) : null}
            </div>
          </div>
          <div className="border-t border-gray-100 dark:border-gray-700/60 mt-3 pt-3">
            {/* 操作行：预览在左、按钮在右 */}
            <div className="flex items-center justify-between gap-3">
              {draft?.from && draft?.to ? (
                <span className="text-xs text-gray-400 dark:text-gray-500 tabular-nums">
                  {format(draft.from, 'MM/dd HH:mm')} ~ {format(draft.to, 'MM/dd HH:mm')}
                </span>
              ) : (
                <span className="text-xs text-gray-400 dark:text-gray-500">在日历选择日期范围</span>
              )}
              <div className="flex items-center gap-2 shrink-0">
                <button
                  type="button"
                  onClick={clear}
                  className="text-xs text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200 border border-gray-200 dark:border-gray-600 rounded-lg px-2.5 py-1.5"
                >
                  清除选择
                </button>
                <button
                  type="button"
                  onClick={cancel}
                  className="text-xs text-gray-600 dark:text-gray-300 hover:text-gray-800 dark:hover:text-gray-100 border border-gray-200 dark:border-gray-600 rounded-lg px-2.5 py-1.5"
                >
                  取消
                </button>
                <button
                  type="button"
                  onClick={confirm}
                  disabled={!draft?.from || !draft?.to}
                  className="text-xs text-white bg-indigo-600 hover:bg-indigo-700 rounded-lg px-3 py-1.5 disabled:opacity-50"
                >
                  确认
                </button>
              </div>
            </div>
          </div>
        </PopoverContent>
      </Popover>
    </div>
  )
}
