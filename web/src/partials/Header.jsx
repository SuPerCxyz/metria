// Metria 顶部栏：页面标题 + 汉堡（移动端）+ 全局时间范围 + 主题切换 + 退出登录。

import React, { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import ThemeToggle from '../components/ThemeToggle'
import TimeRangePicker from '../components/filters/TimeRangePicker'
import { usePageMeta } from '../hooks/usePageMeta'
import { getToken, setToken } from '../services/api'

function Header({ sidebarOpen, setSidebarOpen }) {
  const navigate = useNavigate()
  const [userMenuOpen, setUserMenuOpen] = useState(false)
  const { title, subtitle } = usePageMeta()

  const logout = () => {
    setToken(null)
    window.dispatchEvent(new CustomEvent('metria:unauth'))
    navigate('/login')
  }

  return (
    <header className="sticky top-0 before:absolute before:inset-0 before:backdrop-blur-md max-lg:before:bg-white/90 dark:max-lg:before:bg-gray-800/90 before:-z-10 z-30 max-lg:shadow-xs lg:before:bg-gray-100/90 dark:lg:before:bg-gray-900/90">
      <div className="px-4 sm:px-6 lg:px-8">
        <div className="flex items-center justify-between h-16 lg:border-b border-gray-200 dark:border-gray-700/60">
          <div className="flex items-center gap-3 min-w-0">
            <button
              className="text-gray-500 hover:text-gray-600 dark:hover:text-gray-400 lg:hidden shrink-0"
              aria-controls="sidebar"
              aria-expanded={sidebarOpen}
              onClick={(e) => { e.stopPropagation(); setSidebarOpen(!sidebarOpen) }}
            >
              <span className="sr-only">打开侧边栏</span>
              <svg className="w-6 h-6 fill-current" viewBox="0 0 24 24"><rect x="4" y="5" width="16" height="2" /><rect x="4" y="11" width="16" height="2" /><rect x="4" y="17" width="16" height="2" /></svg>
            </button>
            <div className="min-w-0">
              <h1 className="text-base sm:text-lg font-bold text-gray-800 dark:text-gray-100 truncate leading-tight">{title || 'Metria'}</h1>
              {subtitle && <p className="text-xs text-gray-400 dark:text-gray-500 truncate leading-tight">{subtitle}</p>}
            </div>
          </div>

          <div className="flex items-center space-x-3 shrink-0">
            <TimeRangePicker />
            <ThemeToggle />
            <hr className="w-px h-6 bg-gray-200 dark:bg-gray-700/60 border-none" />
            <div className="relative">
              <button
                className="w-8 h-8 flex items-center justify-center rounded-full bg-gray-200 dark:bg-gray-700 text-gray-600 dark:text-gray-300 text-xs font-bold"
                onClick={() => setUserMenuOpen(!userMenuOpen)}
                aria-label="用户菜单"
              >
                {(getToken() || 'A').slice(0, 1).toUpperCase()}
              </button>
              {userMenuOpen && (
                <div className="absolute right-0 mt-2 w-48 rounded-xl bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700/60 shadow-lg p-2">
                  <button
                    type="button"
                    onClick={logout}
                    className="w-full text-left text-sm text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700/40 rounded-lg px-3 py-2"
                  >
                    退出登录
                  </button>
                </div>
              )}
            </div>
          </div>
        </div>
      </div>
    </header>
  )
}

export default Header
