// Metria 顶部栏：页面标题 + 汉堡（移动端）+ 全局时间范围 + 主题切换 + 退出登录。

import React, { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import ThemeToggle from '../components/ThemeToggle'
import UserAvatar from '../components/common/UserAvatar'
import TimeRangePicker from '../components/filters/TimeRangePicker'
import { usePageMeta } from '../hooks/usePageMeta'
import { useTimeRange } from '../hooks/useTimeRange'
import { api, setToken } from '../services/api'

function Header({ sidebarOpen, setSidebarOpen }) {
  const navigate = useNavigate()
  const [userMenuOpen, setUserMenuOpen] = useState(false)
  const [user, setUser] = useState(null)
  const { subtitle } = usePageMeta()
  const { refreshRange } = useTimeRange()

  useEffect(() => {
    const load = () => api('/auth/me').then(setUser).catch(() => setUser(null))
    const onProfileUpdated = () => load()
    load()
    window.addEventListener('metria:profile-updated', onProfileUpdated)
    return () => window.removeEventListener('metria:profile-updated', onProfileUpdated)
  }, [])

  const logout = () => {
    setToken(null)
    window.dispatchEvent(new CustomEvent('metria:unauth'))
    navigate('/login')
  }

  const refreshPage = () => {
    // 先通知查询记录旧 key，再重算快捷范围；自定义范围不会被改写。
    window.dispatchEvent(new CustomEvent('metria:refresh'))
    refreshRange()
  }

  return (
    <header className="sticky top-0 before:absolute before:inset-0 before:backdrop-blur-md max-lg:before:bg-white/90 dark:max-lg:before:bg-gray-800/90 before:-z-10 z-30 max-lg:shadow-xs lg:before:bg-gray-100/90 dark:lg:before:bg-gray-900/90">
      <div className="px-4 sm:px-6 lg:px-8">
        <div className="flex min-h-16 items-center justify-between gap-3 py-3 sm:py-0 lg:border-b border-gray-200 dark:border-gray-700/60">
          <div className="flex min-w-0 flex-1 items-center gap-3">
            <button
              className="text-gray-500 hover:text-gray-600 dark:hover:text-gray-400 lg:hidden shrink-0"
              aria-controls="sidebar"
              aria-expanded={sidebarOpen}
              onClick={(e) => { e.stopPropagation(); setSidebarOpen(!sidebarOpen) }}
            >
              <span className="sr-only">打开侧边栏</span>
              <svg className="w-6 h-6 fill-current" viewBox="0 0 24 24"><rect x="4" y="5" width="16" height="2" /><rect x="4" y="11" width="16" height="2" /><rect x="4" y="17" width="16" height="2" /></svg>
            </button>
            {subtitle && <p className="min-w-0 truncate text-xs sm:text-sm text-gray-400 dark:text-gray-500">{subtitle}</p>}
          </div>

          <div className="flex min-w-0 items-center gap-2 sm:gap-3 sm:shrink-0 ml-auto">
            <button
              type="button"
              onClick={refreshPage}
              className="w-8 h-8 shrink-0 flex items-center justify-center rounded-full text-gray-500 hover:text-gray-700 hover:bg-gray-100 dark:text-gray-400 dark:hover:text-gray-200 dark:hover:bg-gray-700/60"
              aria-label="刷新页面"
              title="刷新当前页面数据"
            >
              <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M3 12a9 9 0 0 1 15.5-6.3L21 8" />
                <path d="M21 3v5h-5" />
                <path d="M21 12a9 9 0 0 1-15.5 6.3L3 16" />
                <path d="M3 21v-5h5" />
              </svg>
            </button>
            <TimeRangePicker className="min-w-0 flex-1 sm:flex-none" />
            <ThemeToggle />
            <hr className="hidden sm:block w-px h-6 bg-gray-200 dark:bg-gray-700/60 border-none" />
            <div className="relative shrink-0">
              <button
                className="w-8 h-8 flex items-center justify-center rounded-full bg-gray-200 dark:bg-gray-700 text-gray-600 dark:text-gray-300 text-xs font-bold"
                onClick={() => setUserMenuOpen(!userMenuOpen)}
                aria-label="用户菜单"
              >
                <UserAvatar user={user} />
              </button>
              {userMenuOpen && (
                <div className="absolute right-0 mt-2 w-56 rounded-xl bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700/60 shadow-lg p-2">
                  <div className="flex items-center gap-3 px-2 py-2">
                    <UserAvatar user={user} />
                    <div className="min-w-0">
                      <div className="truncate text-sm font-semibold text-gray-700 dark:text-gray-200">{user?.display_name || user?.username || 'Admin'}</div>
                      <div className="truncate text-xs text-gray-400 dark:text-gray-500">{user?.username || 'admin'}</div>
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() => { setUserMenuOpen(false); navigate('/settings?tab=system') }}
                    className="w-full text-left text-sm text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700/40 rounded-lg px-3 py-2"
                  >
                    账户设置
                  </button>
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
