// 状态徽标与数据质量标记。

import React from 'react'
import { statusTone } from '../../services/format'

const toneClass = {
  success: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-400/10 dark:text-emerald-400',
  danger: 'bg-red-100 text-red-700 dark:bg-red-400/10 dark:text-red-400',
  warning: 'bg-amber-100 text-amber-700 dark:bg-amber-400/10 dark:text-amber-400',
  muted: 'bg-gray-100 text-gray-600 dark:bg-gray-700/40 dark:text-gray-400',
}

export default function StatusBadge({ status, dot = true }) {
  const tone = statusTone(status)
  return (
    <span className={`inline-flex items-center gap-1.5 text-xs font-medium px-2 py-0.5 rounded-full ${toneClass[tone] || toneClass.muted}`}>
      {dot && <span className={`w-1.5 h-1.5 rounded-full ${tone === 'success' ? 'bg-emerald-500' : tone === 'danger' ? 'bg-red-500' : tone === 'warning' ? 'bg-amber-500' : 'bg-gray-400'}`} />}
      {status}
    </span>
  )
}

// 数据质量标记：精确值 / 估算值 / 部分缺失
export function DataQualityBadge({ kind }) {
  const map = {
    exact: { label: '精确值', cls: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-400/10 dark:text-emerald-400' },
    estimated: { label: '估算值', cls: 'bg-sky-100 text-sky-700 dark:bg-sky-400/10 dark:text-sky-400' },
    partial: { label: '部分缺失', cls: 'bg-amber-100 text-amber-700 dark:bg-amber-400/10 dark:text-amber-400' },
    missing: { label: '价格未配置', cls: 'bg-gray-100 text-gray-600 dark:bg-gray-700/40 dark:text-gray-400' },
  }
  const m = map[kind] || map.missing
  return <span className={`inline-flex items-center text-xs font-medium px-2 py-0.5 rounded-full ${m.cls}`}>{m.label}</span>
}
