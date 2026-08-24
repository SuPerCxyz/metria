// 用户头像：使用可持久化的文字与颜色键，不加载外部图片。

import React from 'react'

const COLORS = {
  indigo: 'bg-indigo-100 text-indigo-700 dark:bg-indigo-400/20 dark:text-indigo-300',
  emerald: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-400/20 dark:text-emerald-300',
  amber: 'bg-amber-100 text-amber-700 dark:bg-amber-400/20 dark:text-amber-300',
  rose: 'bg-rose-100 text-rose-700 dark:bg-rose-400/20 dark:text-rose-300',
  sky: 'bg-sky-100 text-sky-700 dark:bg-sky-400/20 dark:text-sky-300',
  violet: 'bg-violet-100 text-violet-700 dark:bg-violet-400/20 dark:text-violet-300',
}

export default function UserAvatar({ user, size = 'sm' }) {
  const source = user?.avatar_text || user?.display_name || user?.username || 'A'
  const text = Array.from(source.trim() || 'A').slice(0, 2).join('').toUpperCase()
  const sizeClass = size === 'lg' ? 'h-16 w-16 text-xl' : 'h-8 w-8 text-xs'
  return (
    <span className={`inline-flex shrink-0 items-center justify-center rounded-full font-bold ${sizeClass} ${COLORS[user?.avatar_color] || COLORS.indigo}`}>
      {text}
    </span>
  )
}
