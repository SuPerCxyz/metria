// 通用数据表：列排序、分页、行点击。每页 10-20 行，行高约 52px。

import React, { useMemo, useState } from 'react'
import { cn } from '../../lib/utils'

export default function DataTable({ columns, data, pageSize = 12, onRowClick, emptyText }) {
  const [sortKey, setSortKey] = useState(null)
  const [sortDir, setSortDir] = useState(1)
  const [page, setPage] = useState(0)

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

  const toggleSort = (key) => {
    if (sortKey === key) setSortDir((d) => (d === 1 ? -1 : 1))
    else { setSortKey(key); setSortDir(1) }
  }

  return (
    <div>
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-gray-200 dark:border-gray-700/60">
              {columns.map((col) => (
                <th
                  key={col.key}
                  className={cn('px-3 py-2.5 text-left text-xs font-semibold text-gray-500 dark:text-gray-400 whitespace-nowrap', col.sortable && 'cursor-pointer select-none hover:text-gray-700 dark:hover:text-gray-200')}
                  onClick={col.sortable ? () => toggleSort(col.key) : undefined}
                >
                  <span className="inline-flex items-center gap-1">
                    {col.label}
                    {col.sortable && sortKey === col.key && (sortDir === 1 ? <span className="text-[10px]">▲</span> : <span className="text-[10px]">▼</span>)}
                  </span>
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
                className={cn('border-b border-gray-100 dark:border-gray-800', onRowClick && 'cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-700/20')}
              >
                {columns.map((col) => (
                  <td key={col.key} className="px-3 py-3 whitespace-nowrap text-gray-600 dark:text-gray-300">
                    {col.render ? col.render(row) : String(row[col.key] ?? '—')}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {pages > 1 && (
        <div className="flex items-center justify-between mt-4 px-1">
          <span className="text-xs text-gray-400 dark:text-gray-500 tabular-nums">
            {safePage * pageSize + 1}-{Math.min((safePage + 1) * pageSize, total)} / {total}
          </span>
          <div className="flex gap-1">
            <button type="button" disabled={safePage === 0} onClick={() => setPage(safePage - 1)} className="btn-xs btn">上一页</button>
            <button type="button" disabled={safePage >= pages - 1} onClick={() => setPage(safePage + 1)} className="btn-xs btn">下一页</button>
          </div>
        </div>
      )}
    </div>
  )
}
