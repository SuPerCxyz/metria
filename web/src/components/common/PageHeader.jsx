// 页面头部：标题 + 副标题 + 右侧操作区。

import React from 'react'

export default function PageHeader({ title, subtitle, actions, back }) {
  return (
    <div className="sm:flex sm:justify-between sm:items-center mb-8">
      <div className="mb-4 sm:mb-0">
        {back && <div className="mb-2">{back}</div>}
        <h1 className="text-2xl md:text-3xl text-gray-800 dark:text-gray-100 font-bold">{title}</h1>
        {subtitle && <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">{subtitle}</p>}
      </div>
      {actions && <div className="grid grid-flow-col sm:auto-cols-max justify-start sm:justify-end gap-2">{actions}</div>}
    </div>
  )
}
