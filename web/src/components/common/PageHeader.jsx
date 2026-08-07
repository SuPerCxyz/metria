// 页面头部：标题注册到顶栏（Header 显示），正文顶部保留返回/操作区。

import React, { useEffect } from 'react'
import { usePageMeta } from '../../hooks/usePageMeta'

export default function PageHeader({ title, subtitle, actions, back }) {
  const { setMeta } = usePageMeta()

  useEffect(() => {
    setMeta({ title, subtitle: subtitle || null })
  }, [title, subtitle, setMeta])

  if (!actions && !back) return null
  return (
    <div className="flex items-center justify-between mb-6">
      <div>{back}</div>
      {actions && <div className="grid grid-flow-col sm:auto-cols-max justify-start sm:justify-end gap-2">{actions}</div>}
    </div>
  )
}
