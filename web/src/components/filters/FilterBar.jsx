// 筛选栏：时间范围（全局已含）+ 主要维度 + 搜索 + 更多筛选。

import React, { useState } from 'react'

export default function FilterBar({ searchPlaceholder, onSearch, primary, moreFields }) {
  const [search, setSearch] = useState('')
  const [moreOpen, setMoreOpen] = useState(false)

  const doSearch = (e) => {
    const v = e.target.value
    setSearch(v)
    onSearch?.(v)
  }

  return (
    <div className="flex flex-wrap items-center gap-3 mb-6">
      {primary}
      <div className="relative flex-1 min-w-[200px] max-w-xs">
        <svg className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 fill-current text-gray-400" viewBox="0 0 16 16">
          <path d="M7 14c-3.86 0-7-3.14-7-7s3.14-7 7-7 7 3.14 7 7-3.14 7-7 7ZM7 2C4.243 2 2 4.243 2 7s2.243 5 5 5 5-2.243 5-5-2.243-5-5-5Z" />
          <path d="m13.314 11.9 2.393 2.393a.999.999 0 1 1-1.414 1.414L11.9 13.314a8.019 8.019 0 0 0 1.414-1.414Z" />
        </svg>
        <input
          type="text"
          value={search}
          onChange={doSearch}
          placeholder={searchPlaceholder || '搜索…'}
          className="w-full pl-9 pr-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm text-gray-700 dark:text-gray-200 focus:ring-2 focus:ring-indigo-500/30 focus:border-indigo-500"
        />
      </div>
      {moreFields && (
        <>
          <button type="button" onClick={() => setMoreOpen(!moreOpen)} className="btn">
            更多筛选
            <svg className="ml-1 fill-current text-gray-400 w-3 h-3" viewBox="0 0 12 12"><path d="M6 8.8 1.2 4h9.6L6 8.8Z" /></svg>
          </button>
          {moreOpen && <div className="w-full">{moreFields}</div>}
        </>
      )}
    </div>
  )
}
