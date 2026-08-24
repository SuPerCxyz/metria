// 通用数据表：列排序、分页、行点击。每页 10-20 行，行高约 52px。

import React, { useEffect, useMemo, useRef, useState } from 'react'
import { cn } from '../../lib/utils'

export default function DataTable({ columns, data, pageSize = 12, onRowClick, emptyText }) {
  const [sortKey, setSortKey] = useState(null)
  const [sortDir, setSortDir] = useState(1)
  const [page, setPage] = useState(0)
  const scrollRef = useRef(null)
  const [scrollHint, setScrollHint] = useState({ left: false, right: false })

  const rows = useMemo(() => {
    let list = data || []
    if (sortKey) {
      list = [...list].sort((a, b) => {
        const av = a[sortKey]
        const bv = b[sortKey]
        const cmp = typeof av === 'number' ? av - (bv ?? 0) : String(av ?? '').localeCompare(String(bv ?? ''))
        return cmp * sortDir
      })
    }
    return list
  }, [data, sortKey, sortDir])

  const total = rows.length
  const pages = Math.max(1, Math.ceil(total / pageSize))
  const safePage = Math.min(page, pages - 1)
  const pageRows = rows.slice(safePage * pageSize, safePage * pageSize + pageSize)

  useEffect(() => setPage(0), [total])

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
  }, [data, columns.length, pageSize])

  const toggleSort = (key) => {
    if (sortKey === key) setSortDir((d) => (d === 1 ? -1 : 1))
    else { setSortKey(key); setSortDir(1) }
  }

  return (
    <div>
      <div
        ref={scrollRef}
        role="region"
        aria-label="数据表格，可横向滚动查看更多列"
        tabIndex={0}
        className="relative overflow-x-auto [scrollbar-width:thin]"
      >
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-gray-200 dark:border-gray-700/60">
              {columns.map((col) => (
                <th
                  key={col.key}
                  scope="col"
                  aria-sort={col.sortable ? (sortKey === col.key ? (sortDir === 1 ? 'ascending' : 'descending') : 'none') : undefined}
                  className={cn(
                    'px-3 py-2.5 text-left text-xs font-semibold text-gray-500 dark:text-gray-400 whitespace-nowrap',
                    col.headerClassName
                  )}
                >
                  {col.sortable ? (
                    <button
                      type="button"
                      onClick={() => toggleSort(col.key)}
                      className="inline-flex items-center gap-1 rounded-sm hover:text-gray-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-500 dark:hover:text-gray-200"
                    >
                      {col.label}
                      {sortKey === col.key && (sortDir === 1 ? <span aria-hidden="true" className="text-xs leading-none">▲</span> : <span aria-hidden="true" className="text-xs leading-none">▼</span>)}
                    </button>
                  ) : <span>{col.label}</span>}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {pageRows.length === 0 && (
              <tr><td colSpan={columns.length} className="py-12 text-center text-gray-400 dark:text-gray-500">{emptyText || '暂无数据'}</td></tr>
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
                {columns.map((col) => (
                  <td key={col.key} className={cn('px-3 py-3 whitespace-nowrap text-gray-600 dark:text-gray-300', col.cellClassName)}>
                    {col.render ? col.render(row) : String(row[col.key] ?? '—')}
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
    </div>
  )
}
