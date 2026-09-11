// 用户头像：OIDC HTTPS picture 优先，失败时回退到可持久化文字与颜色。

import React, { useEffect, useState } from 'react'

const COLORS = {
  indigo: 'bg-indigo-100 text-indigo-700 dark:bg-indigo-400/20 dark:text-indigo-300',
  emerald: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-400/20 dark:text-emerald-300',
  amber: 'bg-amber-100 text-amber-700 dark:bg-amber-400/20 dark:text-amber-300',
  rose: 'bg-rose-100 text-rose-700 dark:bg-rose-400/20 dark:text-rose-300',
  sky: 'bg-sky-100 text-sky-700 dark:bg-sky-400/20 dark:text-sky-300',
  violet: 'bg-violet-100 text-violet-700 dark:bg-violet-400/20 dark:text-violet-300',
}

export default function UserAvatar({ user, size = 'sm' }) {
  const [imageFailed, setImageFailed] = useState(false)
  const avatarUrl = typeof user?.avatar_url === 'string' && user.avatar_url.startsWith('https://')
    ? user.avatar_url
    : ''
  useEffect(() => setImageFailed(false), [avatarUrl])
  const source = user?.avatar_text || user?.display_name || user?.username || 'A'
  const text = Array.from(source.trim() || 'A').slice(0, 2).join('').toUpperCase()
  const sizeClass = size === 'lg' ? 'h-16 w-16 text-xl' : 'h-8 w-8 text-xs'
  const label = `${user?.display_name || user?.username || '用户'}的头像`
  if (avatarUrl && !imageFailed) {
    return (
      <img
        src={avatarUrl}
        alt={label}
        referrerPolicy="no-referrer"
        onError={() => setImageFailed(true)}
        className={`${sizeClass} shrink-0 rounded-full object-cover`}
      />
    )
  }
  return (
    <span role="img" aria-label={label} className={`inline-flex shrink-0 items-center justify-center rounded-full font-bold ${sizeClass} ${COLORS[user?.avatar_color] || COLORS.indigo}`}>
      {text}
    </span>
  )
}
