// 状态徽标与数据质量标记。

import React from 'react'
import { statusTone } from '../../services/format'

const toneClass = {
  success: 'bg-emerald-500 text-white',
  danger: 'bg-red-500 text-white',
  warning: 'bg-amber-500 text-white',
  muted: 'bg-gray-500 text-white',
}

// 会话状态统一中文展示；调用/节点等其他状态保持原样。
const STATUS_LABELS = {
  active: '活跃',
  idle: '闲置',
  ended: '已结束',
}

export default function StatusBadge({ status, dot = true }) {
  const raw = String(status ?? '')
  const label = STATUS_LABELS[raw] ?? raw
  const tone = statusTone(raw)
  return (
    <span className={`inline-flex items-center gap-1.5 text-xs font-medium px-2 py-0.5 rounded-full ${toneClass[tone] || toneClass.muted}`}>
      {dot && <span className="w-1.5 h-1.5 rounded-full bg-white/80" />}
      {label}
    </span>
  )
}

// 数据质量标记：精确值 / 估算值 / 部分缺失
export function DataQualityBadge({ kind }) {
  const map = {
    exact: { label: '精确值', cls: 'bg-emerald-500 text-white' },
    estimated: { label: '估算值', cls: 'bg-sky-500 text-white' },
    partial: { label: '部分缺失', cls: 'bg-amber-500 text-white' },
    missing: { label: '价格未配置', cls: 'bg-gray-500 text-white' },
  }
  const m = map[kind] || map.missing
  return <span className={`inline-flex items-center text-xs font-medium px-2 py-0.5 rounded-full ${m.cls}`}>{m.label}</span>
}
