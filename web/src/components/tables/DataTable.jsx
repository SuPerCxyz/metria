// 通用数据表：列排序、分页、行点击。列宽由完整数据测量后固定，排序/翻页不改变列宽。

import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { cn } from '../../lib/utils'
import { sortItems } from '../../services/sorting.js'
import { getVisibleColumns } from '../../services/dataAvailability.js'

/// 时间列识别：默认按时间倒序（新→旧）排列。
const TIME_KEY_PATTERNS = [/_at$/, /^timestamp$/, /_time$/, /^bucket$/, /_from$/]

function detectTimeSortKey(columns) {
  for (const col of columns || []) {
    if (col.sortable === false) continue
    if (TIME_KEY_PATTERNS.some((re) => re.test(col.key))) return col.key
  }
  return null
}

/// 测量列宽时的最大采样行数：列宽取各列内容最大值，采样即可代表整体。
const MEASURE_ROWS = 500

const HEADER_CLASS = 'px-3 py-2.5 text-left text-xs font-semibold text-gray-500 dark:text-gray-400 whitespace-nowrap'
const CELL_CLASS = 'px-3 py-3 whitespace-nowrap text-gray-600 dark:text-gray-300'

export default function DataTable({ columns, data, pageSize = 12, onRowClick, emptyText }) {
  const [sortKey, setSortKey] = useState(() => detectTimeSortKey(columns))
  const [sortDir, setSortDir] = useState(-1)
  const [page, setPage] = useState(0)
  const [colWidths, setColWidths] = useState(null)
  const scrollRef = useRef(null)
  const measureRef = useRef(null)
  const [scrollHint, setScrollHint] = useState({ left: false, right: false })
  const visibleColumns = useMemo(() => getVisibleColumns(columns, data), [columns, data])

  const rows = useMemo(() => {
    let list = data || []
    if (!sortKey) return list
    const column = columns.find((item) => item.key === sortKey)
    list = sortItems(list, (row) => column?.sortValue ? column.sortValue(row) : row?.[sortKey], sortDir)
    return list
  }, [columns, data, sortKey, sortDir])

  const total = rows.length
  const pages = Math.max(1, Math.ceil(total / pageSize))
  const safePage = Math.min(page, pages - 1)
  const pageRows = rows.slice(safePage * pageSize, safePage * pageSize + pageSize)

  useEffect(() => setPage(0), [sortKey, sortDir, total])

  // 用完整数据集（有界采样）测量列宽；只在数据或列变化时重算，
  // 因此点击排序/翻页时列宽保持完全一致。
  useLayoutEffect(() => {
    const table = measureRef.current
    if (!table || !data?.length || visibleColumns.length === 0) {
      setColWidths(null)
      return
    }
    const headerCells = [...table.querySelectorAll('thead th')]
    const widths = headerCells.map((cell) => cell.getBoundingClientRect().width)
    if (widths.length !== visibleColumns.length || !widths.every((w) => w > 0)) return
    setColWidths((prev) => (
      prev && prev.length === widths.length && prev.every((w, i) => Math.abs(w - widths[i]) < 0.5)
        ? prev
        : widths
    ))
  }, [data, visibleColumns])

  useEffect(() => {
    const element = scrollRef.current
    if (!element) return undefined
    const updateScrollHint = () => {
      const maxScroll = element.scrollWidth - element.clientWidth
      setScrollHint({
        left: element.scrollLeft > 1,
        right: maxScroll - element.scrollLeft > 1,
      })
    }
    updateScrollHint()
    element.addEventListener('scroll', updateScrollHint, { passive: true })
    window.addEventListener('resize', updateScrollHint)
    return () => {
      element.removeEventListener('scroll', updateScrollHint)
      window.removeEventListener('resize', updateScrollHint)
    }
  }, [data, columns.length, pageSize, colWidths])

  const toggleSort = (key) => {
    if (sortKey === key) setSortDir((d) => (d === 1 ? -1 : 1))
    else { setSortKey(key); setSortDir(1) }
    setPage(0)
  }

  const headerContent = (col) => (col.sortable !== false ? (
    <button
      type="button"
      onClick={() => toggleSort(col.key)}
      aria-label={`${col.label}，${sortKey === col.key ? (sortDir === 1 ? '当前升序，再次点击降序' : '当前降序，再次点击升序') : '按升序排序'}`}
      className="inline-flex items-center gap-1 rounded-sm hover:text-gray-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-500 dark:hover:text-gray-200"
    >
      {col.label}
      <span aria-hidden="true" className="text-[10px] leading-none text-gray-400 dark:text-gray-500">{sortKey === col.key && sortDir === 1 ? '▲' : '▼'}</span>
    </button>
  ) : <span>{col.label}</span>)

  const cellContent = (col, row) => (col.render ? col.render(row) : String(row[col.key] ?? '—'))

  const tableStyle = colWidths
    ? { tableLayout: 'fixed', minWidth: `${Math.ceil(colWidths.reduce((sum, w) => sum + w, 0))}px` }
    : undefined

  return (
    <div>
      <div
        ref={scrollRef}
        role="region"
        aria-label="数据表格，可横向滚动查看更多列"
        tabIndex={0}
        className="relative overflow-x-auto [scrollbar-width:thin]"
      >
        <table className="w-full text-sm" style={tableStyle}>
          {colWidths && (
            <colgroup>
              {visibleColumns.map((col, index) => (
                <col key={col.key} style={{ width: `${Math.ceil(colWidths[index] ?? 0)}px` }} />
              ))}
            </colgroup>
          )}
          <thead>
            <tr className="border-b border-gray-200 dark:border-gray-700/60">
              {visibleColumns.map((col) => (
                <th
                  key={col.key}
                  scope="col"
                  aria-sort={col.sortable !== false ? (sortKey === col.key ? (sortDir === 1 ? 'ascending' : 'descending') : 'none') : undefined}
                  className={cn(HEADER_CLASS, col.headerClassName)}
                >
                  {headerContent(col)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {pageRows.length === 0 && (
              <tr><td colSpan={visibleColumns.length} className="py-12 text-center text-gray-400 dark:text-gray-500">{emptyText || '暂无数据'}</td></tr>
            )}
            {pageRows.map((row, i) => (
              <tr
                key={row.id || i}
                onClick={() => onRowClick?.(row)}
                onKeyDown={onRowClick ? (event) => {
                  if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault()
                    onRowClick(row)
                  }
                } : undefined}
                tabIndex={onRowClick ? 0 : undefined}
                className={cn('border-b border-gray-100 dark:border-gray-800', onRowClick && 'cursor-pointer hover:bg-gray-50 focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-indigo-500 dark:hover:bg-gray-700/20')}
              >
                {visibleColumns.map((col) => (
                  <td key={col.key} className={cn(CELL_CLASS, col.cellClassName)}>
                    {cellContent(col, row)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
        {scrollHint.right && (
          <div className="pointer-events-none absolute right-2 bottom-2 z-10 rounded-full border border-gray-200 bg-white/95 px-2.5 py-1 text-[10px] font-medium text-gray-500 shadow-sm dark:border-gray-600 dark:bg-gray-800/95 dark:text-gray-300">
            横向滚动 →
          </div>
        )}
      </div>
      {pages > 1 && (
        <div className="flex items-center justify-between mt-4 px-1">
          <span className="text-xs text-gray-400 dark:text-gray-500 tabular-nums">
            {safePage * pageSize + 1}-{Math.min((safePage + 1) * pageSize, total)} / {total}
          </span>
          <div className="flex gap-2">
            <button type="button" disabled={safePage === 0} onClick={() => setPage(safePage - 1)} className="btn-xs btn-secondary">上一页</button>
            <button type="button" disabled={safePage >= pages - 1} onClick={() => setPage(safePage + 1)} className="btn-xs btn-secondary">下一页</button>
          </div>
        </div>
      )}

      {/* 离屏测量表：用完整数据（有界采样）确定各列自然宽度，仅用于测量，不参与交互与朗读 */}
      <div
        aria-hidden="true"
        inert
        style={{ position: 'fixed', left: 0, top: 0, visibility: 'hidden', pointerEvents: 'none', zIndex: -1, width: 'max-content' }}
      >
        <table ref={measureRef} className="text-sm" style={{ tableLayout: 'auto', width: 'max-content' }}>
          <thead>
            <tr>
              {visibleColumns.map((col) => (
                <th key={col.key} scope="col" className={cn(HEADER_CLASS, col.headerClassName)}>{headerContent(col)}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {(data || []).slice(0, MEASURE_ROWS).map((row, i) => (
              <tr key={row?.id || i}>
                {visibleColumns.map((col) => (
                  <td key={col.key} className={cn(CELL_CLASS, col.cellClassName)}>{cellContent(col, row)}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}
