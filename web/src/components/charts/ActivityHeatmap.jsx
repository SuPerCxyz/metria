// 星期 × 小时活跃热力图，单元格点击由调用方负责下钻。

import React, { useMemo } from 'react'
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

export default function ActivityHeatmap({ cells = [], metric, onMetricChange, onCellClick, loading = false, error = null }) {
  const byIndex = useMemo(() => new Map(cells.map((cell) => [cell.weekday * 24 + cell.hour, cell])), [cells])
  const max = useMemo(() => Math.max(...cells.map((cell) => valueOf(cell, metric) || 0), 0), [cells, metric])

  return (
    <div className="flex h-full flex-col">
      <div className="mb-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">小时活跃热力图</h2>
          <Segmented items={METRICS} value={metric} onChange={onMetricChange} />
        </div>
        <p className="mt-1 text-xs text-gray-400 dark:text-gray-500">按展示时区聚合；点击有数据的单元格下钻到最近匹配小时</p>
      </div>
      {loading && cells.length === 0 ? <div className="flex flex-1 items-center justify-center py-12 text-center text-sm text-gray-400 dark:text-gray-500">加载中…</div> : error ? <div className="flex flex-1 items-center justify-center py-12 text-center text-sm text-amber-600 dark:text-amber-400">热力图加载失败，请刷新重试。</div> : <div className="flex flex-1 items-center overflow-x-auto pb-1">
        <div className="grid w-full grid-rows-[auto_repeat(7,auto)] grid-cols-[2.5rem_repeat(24,minmax(0.75rem,1fr))] gap-1 text-[10px] text-gray-400 dark:text-gray-500">
          <span aria-hidden="true" />
          {Array.from({ length: 24 }, (_, hour) => <span key={hour} className="text-center tabular-nums">{hour}</span>)}
          {WEEKDAYS.map((weekday, day) => (
            <React.Fragment key={weekday}>
              <span className="flex items-center pr-1 text-right">{weekday}</span>
              {Array.from({ length: 24 }, (_, hour) => {
                const cell = byIndex.get(day * 24 + hour) || { weekday: day, hour, tokens: 0, cost_micro_usd: 0, model_calls: 0, duration_ms: null }
                const value = valueOf(cell, metric)
                const alpha = max > 0 && value > 0 ? 0.12 + (value / max) * 0.78 : 0
                const label = `${weekday}${hour}时：${formatValue(value, metric)}${cell.latest_from ? `，最近 ${fmtDateTime(cell.latest_from)}` : ''}`
                return (
                  <button
                    key={`${day}-${hour}`}
                    type="button"
                    disabled={!cell.latest_from || !cell.latest_to}
                    onClick={() => onCellClick?.(cell)}
                    title={label}
                    aria-label={label}
                    className="aspect-square w-full rounded-sm border border-transparent transition hover:border-indigo-400 disabled:cursor-default"
                    style={alpha ? { backgroundColor: `rgba(99, 102, 241, ${alpha})` } : undefined}
                  />
                )
              })}
            </React.Fragment>
          ))}
        </div>
      </div>}
      {!error && !(loading && cells.length === 0) && <div className="mt-3 flex items-center justify-end gap-2 text-[10px] text-gray-400 dark:text-gray-500">
        <span>低</span><span className="h-2.5 w-8 rounded-sm bg-indigo-100 dark:bg-indigo-400/20" /><span className="h-2.5 w-8 rounded-sm bg-indigo-500" /><span>高</span>
      </div>}
    </div>
  )
}
