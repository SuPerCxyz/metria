// 自定义时间选择下拉：点击时间按钮弹出时/分列表，选择即生效，无时钟图标与黑框。
// 下拉通过 portal 渲染到 body，避免受外层容器 overflow 影响出现滚动条。

import React from 'react'
import { createPortal } from 'react-dom'
import { cn } from '../../lib/utils'

function pad(n) {
  return String(n).padStart(2, '0')
}

const HOURS = Array.from({ length: 24 }, (_, i) => pad(i))
const MINUTES = Array.from({ length: 60 }, (_, i) => pad(i))

export default function TimePicker({ value = '00:00', onChange }) {
  const [open, setOpen] = React.useState(false)
  const rootRef = React.useRef(null)
  const dropRef = React.useRef(null)
  const [pos, setPos] = React.useState(null)

  const updatePos = React.useCallback(() => {
    const el = rootRef.current
    if (!el) return
    const r = el.getBoundingClientRect()
    const pw = document.documentElement.clientWidth
    const ph = document.documentElement.clientHeight
    const w = 112
    const h = 158
    let left = r.left
    if (left + w > pw - 8) left = Math.max(8, pw - 8 - w)
    let top = r.bottom + 4
    if (top + h > ph - 8) top = Math.max(8, r.top - 4 - h)
    setPos({ left, top })
  }, [])

  React.useEffect(() => {
    if (!open) return
    updatePos()
    const onDocMouseDown = (e) => {
      const inRoot = rootRef.current && rootRef.current.contains(e.target)
      const inDrop = dropRef.current && dropRef.current.contains(e.target)
      if (!inRoot && !inDrop) {
        setOpen(false)
      }
    }
    const onWinScroll = () => updatePos()
    document.addEventListener('mousedown', onDocMouseDown)
    window.addEventListener('scroll', onWinScroll, true)
    window.addEventListener('resize', onWinScroll)
    return () => {
      document.removeEventListener('mousedown', onDocMouseDown)
      window.removeEventListener('scroll', onWinScroll, true)
      window.removeEventListener('resize', onWinScroll)
    }
  }, [open, updatePos])

  const [hh, mm] = value.split(':')

  const dropdown = (
    <div
      ref={dropRef}
      className="fixed z-[60] mt-0 flex gap-1.5 rounded-lg border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-800 p-1.5 shadow-lg"
      style={{ left: pos?.left, top: pos?.top }}
    >
      <TimeColumn label="时" items={HOURS} selected={hh} onPick={(h) => onChange(`${h}:${mm}`)} />
      <TimeColumn
        label="分"
        items={MINUTES}
        selected={mm}
        onPick={(m) => {
          onChange(`${hh}:${m}`)
          setOpen(false)
        }}
      />
    </div>
  )

  return (
    <div className="relative" ref={rootRef}>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className={cn(
          'tabular-nums text-xs px-1.5 py-1 rounded transition-colors focus:outline-none',
          open
            ? 'bg-violet-50 dark:bg-violet-400/20 text-violet-700 dark:text-violet-300'
            : 'text-gray-700 dark:text-gray-200 hover:bg-gray-100 dark:hover:bg-gray-700/40'
        )}
      >
        {value}
      </button>
      {open && pos ? createPortal(dropdown, document.body) : null}
    </div>
  )
}

function TimeColumn({ label, items, selected, onPick }) {
  const listRef = React.useRef(null)

  React.useEffect(() => {
    listRef.current
      ?.querySelector('[data-selected="true"]')
      ?.scrollIntoView({ block: 'center' })
  }, [])

  return (
    <div className="flex flex-col items-center">
      <div className="text-[10px] text-gray-400 dark:text-gray-500 pb-0.5 leading-none">{label}</div>
      <div ref={listRef} className="h-32 overflow-y-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {items.map((it) => (
          <button
            key={it}
            type="button"
            data-selected={it === selected}
            onClick={() => onPick(it)}
            className={cn(
              'block w-10 text-center text-xs py-1 rounded focus:outline-none',
              it === selected
                ? 'bg-violet-500 text-white'
                : 'text-gray-600 dark:text-gray-300 hover:bg-violet-100 dark:hover:bg-violet-400/20'
            )}
          >
            {it}
          </button>
        ))}
      </div>
    </div>
  )
}
