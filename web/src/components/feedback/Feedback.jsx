// 反馈组件：空状态 / 错误状态 / 加载骨架 / 数据质量提示。

import React from 'react'

export function EmptyState({ title = '暂无数据', desc, icon }) {
  return (
    <div className="flex flex-col items-center justify-center py-12 text-center">
      <div className="w-12 h-12 mb-3 flex items-center justify-center text-gray-300 dark:text-gray-600">
        {icon || (
          <svg className="w-8 h-8" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M3 10h18M3 6h18M5 14h8m-8 4h5" strokeLinecap="round" />
          </svg>
        )}
      </div>
      <div className="text-sm font-medium text-gray-500 dark:text-gray-400">{title}</div>
      {desc && <div className="mt-1 text-xs text-gray-400 dark:text-gray-500 max-w-xs">{desc}</div>}
    </div>
  )
}

export function ErrorState({ error, onRetry }) {
  const msg = error?.message || String(error || '加载失败')
  return (
    <div className="flex flex-col items-center justify-center py-12 text-center">
      <div className="w-12 h-12 mb-3 flex items-center justify-center text-red-400">
        <svg className="w-8 h-8" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
          <path d="M12 9v4m0 4h.01M10.3 3.9 2.4 17a2 2 0 0 0 1.7 3h15.8a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </div>
      <div className="text-sm font-medium text-gray-600 dark:text-gray-300">{msg}</div>
      {onRetry && (
        <button type="button" onClick={onRetry} className="mt-3 text-sm text-indigo-600 dark:text-indigo-400 hover:underline">
          重试
        </button>
      )}
    </div>
  )
}

export function LoadingSkeleton({ rows = 5 }) {
  return (
    <div className="space-y-3 animate-pulse">
      {Array.from({ length: rows }).map((_, i) => (
        <div key={i} className="h-10 bg-gray-100 dark:bg-gray-700/40 rounded-lg" />
      ))}
    </div>
  )
}

export function DataQualityNote({ text, kind = 'estimated' }) {
  const colors = {
    estimated: 'text-sky-600 dark:text-sky-400 border-sky-200 dark:border-sky-800 bg-sky-50 dark:bg-sky-400/5',
    exact: 'text-emerald-600 dark:text-emerald-400 border-emerald-200 dark:border-emerald-800 bg-emerald-50 dark:bg-emerald-400/5',
    partial: 'text-amber-600 dark:text-amber-400 border-amber-200 dark:border-amber-800 bg-amber-50 dark:bg-amber-400/5',
  }
  return (
    <div className={`text-xs px-3 py-2 rounded-lg border ${colors[kind] || colors.estimated}`}>
      {text}
    </div>
  )
}
