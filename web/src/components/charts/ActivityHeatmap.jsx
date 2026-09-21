// 星期 × 小时活跃热力图，单元格点击由调用方负责下钻；悬浮显示该时段 Token 与请求数。

import React, { useLayoutEffect, useMemo, useRef, useState } from 'react'
import Segmented from '../ui/Segmented'
import { fmtDateTime, fmtDuration, fmtTokensShort, fmtUsd } from '../../services/format'

const METRICS = [
  { key: 'tokens', label: 'Token' },
  { key: 'cost', label: '费用' },
  { key: 'requests', label: '请求' },
  { key: 'duration', label: '时长' },
]

const WEEKDAYS = ['周一', '周二', '周三', '周四', '周五', '周六', '周日']

function valueOf(cell, metric) {
  if (metric === 'cost') return cell.cost_micro_usd ?? null
  if (metric === 'requests') return cell.model_calls ?? 0
  if (metric === 'duration') return cell.duration_ms ?? null
  return cell.tokens ?? 0
}

function formatValue(value, metric) {
  if (value === null || value === undefined) return '—'
  if (metric === 'cost') return fmtUsd(value)
  if (metric === 'duration') return fmtDuration(value)
  if (metric === 'requests') return Number(value).toLocaleString()
  return fmtTokensShort(value)
}

const hourRange = (hour) => `${String(hour).padStart(2, '0')}:00–${String(hour).padStart(2, '0')}:59`

export default function ActivityHeatmap({ cells = [], metric, onMetricChange, onCellClick, selectedCell = null, loading = false, error = null }) {
  const byIndex = useMemo(() => new Map(cells.map((cell) => [cell.weekday * 24 + cell.hour, cell])), [cells])
  const max = useMemo(() => Math.max(...cells.map((cell) => valueOf(cell, metric) || 0), 0), [cells, metric])
  const rootRef = useRef(null)
  const tooltipRef = useRef(null)
  const [hover, setHover] = useState(null)

  // 提示挂在组件根节点下、位于横向滚动容器之外，避免向上超出被 overflow 裁剪。
  const showTooltip = (event, cell, weekday) => {
    const root = rootRef.current
    if (!root) return
    const rect = event.currentTarget.getBoundingClientRect()
    const rootRect = root.getBoundingClientRect()
    setHover({
      cell,
      weekday,
      left: rect.left - rootRect.left + rect.width / 2,
      top: rect.top - rootRect.top,
    })
  }

  // 按提示实际宽度夹取水平位置，避免靠近 0/23 点时越出组件被页面横向裁剪。
  useLayoutEffect(() => {
    const root = rootRef.current
    const tip = tooltipRef.current
    if (!root || !tip || !hover) return
    const half = tip.offsetWidth / 2
    const min = half + 4
    const max = Math.max(min, root.clientWidth - half - 4)
    const clamped = Math.min(Math.max(hover.left, min), max)
    if (Math.abs(clamped - hover.left) > 0.5) {
      setHover((current) => (current ? { ...current, left: clamped } : current))
    }
  }, [hover])

  const tooltip = hover && (
    <div
      ref={tooltipRef}
      className="pointer-events-none absolute z-20 -translate-x-1/2 -translate-y-full whitespace-nowrap rounded-lg border border-gray-200 bg-white px-3 py-2 text-xs shadow-lg dark:border-gray-700 dark:bg-gray-800"
      style={{ left: hover.left, top: hover.top - 6 }}
      role="tooltip"
    >
      <div className="font-medium text-gray-700 dark:text-gray-200">
        {hover.weekday} {hourRange(hover.cell.hour)}
      </div>
      {hover.cell.latest_from ? (
        <div className="mt-1 space-y-0.5 tabular-nums text-gray-500 dark:text-gray-400">
          <div>Token {fmtTokensShort(hover.cell.tokens)}</div>
          <div>请求 {Number(hover.cell.model_calls ?? 0).toLocaleString()}</div>
          {Number(hover.cell.cost_micro_usd ?? 0) > 0 && <div>费用 {fmtUsd(hover.cell.cost_micro_usd)}</div>}
          {hover.cell.duration_ms != null && <div>时长 {fmtDuration(hover.cell.duration_ms)}</div>}
        </div>
      ) : (
        <div className="mt-1 text-gray-400 dark:text-gray-500">本周该时段无数据</div>
      )}
    </div>
  )

  return (
    <div className="relative flex h-full flex-col" ref={rootRef}>
      <div className="mb-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">小时活跃热力图</h2>
          <Segmented items={METRICS} value={metric} onChange={onMetricChange} />
        </div>
        <p className="mt-1 text-xs text-gray-400 dark:text-gray-500">固定显示本周；悬浮查看该时段 Token 与请求数，点击有数据的单元格查看详情</p>
      </div>
      {loading && cells.length === 0 ? <div className="flex flex-1 items-center justify-center py-12 text-center text-sm text-gray-400 dark:text-gray-500">加载中…</div> : error ? <div className="flex flex-1 items-center justify-center py-12 text-center text-sm text-amber-600 dark:text-amber-400">热力图加载失败，请刷新重试。</div> : <div className="relative flex-1 overflow-x-auto pb-1" onMouseLeave={() => setHover(null)}>
        <div className="grid h-full w-full grid-rows-[auto_repeat(7,1fr)] grid-cols-[2.5rem_repeat(24,minmax(0.75rem,1fr))] gap-1 text-[10px] text-gray-400 dark:text-gray-500">
          <span aria-hidden="true" />
          {Array.from({ length: 24 }, (_, hour) => <span key={hour} className="text-center tabular-nums">{hour}</span>)}
          {WEEKDAYS.map((weekday, day) => (
            <React.Fragment key={weekday}>
              <span className="flex items-center pr-1 text-right">{weekday}</span>
              {Array.from({ length: 24 }, (_, hour) => {
                const cell = byIndex.get(day * 24 + hour) || { weekday: day, hour, tokens: 0, cost_micro_usd: 0, model_calls: 0, duration_ms: null }
                const value = valueOf(cell, metric)
                const alpha = max > 0 && value > 0 ? 0.12 + (value / max) * 0.78 : 0
                const label = `${weekday} ${hourRange(hour)}：Token ${fmtTokensShort(cell.tokens)}，请求 ${Number(cell.model_calls ?? 0).toLocaleString()}${cell.latest_from ? `，最近 ${fmtDateTime(cell.latest_from)}` : '（本周无数据）'}`
                const selected = selectedCell?.weekday === day && selectedCell?.hour === hour
                return (
                  <button
                    key={`${day}-${hour}`}
                    type="button"
                    disabled={!cell.latest_from || !cell.latest_to}
                    onClick={() => onCellClick?.(cell)}
                    onMouseEnter={(event) => showTooltip(event, cell, weekday)}
                    onFocus={(event) => showTooltip(event, cell, weekday)}
                    onBlur={() => setHover(null)}
                    aria-label={label}
                    className={`aspect-square w-[78%] justify-self-center self-center rounded-sm border transition hover:border-indigo-400 disabled:cursor-default ${selected ? 'border-indigo-600 ring-2 ring-indigo-300 dark:border-indigo-300 dark:ring-indigo-500/60' : 'border-transparent'}`}
                    style={alpha ? { backgroundColor: `rgba(99, 102, 241, ${alpha})` } : undefined}
                  />
                )
              })}
            </React.Fragment>
          ))}
        </div>
      </div>}
      {!error && !(loading && cells.length === 0) && tooltip}
    </div>
  )
}
