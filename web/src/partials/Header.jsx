// Metria 顶部栏：页面标题 + 汉堡（移动端）+ 全局时间范围 + 主题切换 + 退出登录。

import React, { useCallback, useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import ThemeToggle from '../components/ThemeToggle'
import UserAvatar from '../components/common/UserAvatar'
import TimeRangePicker from '../components/filters/TimeRangePicker'
import UsageFilterPicker from '../components/filters/UsageFilterPicker'
import { usePageMeta } from '../hooks/usePageMeta'
import { beginRefreshWindow, endRefreshWindow, useQueryActivity } from '../hooks/useQuery'
import { useTimeRange } from '../hooks/useTimeRange'
import { api, setToken } from '../services/api'
import { useToast } from '../components/feedback/Toast'

function Header({ sidebarOpen, setSidebarOpen }) {
  const navigate = useNavigate()
  const [userMenuOpen, setUserMenuOpen] = useState(false)
  const [user, setUser] = useState(null)
  const { subtitle } = usePageMeta()
  const { refreshRange } = useTimeRange()
  const [refreshing, setRefreshing] = useState(false)
  // 旋转角度按整圈累积：结束时停在 360 的整数倍，即默认朝向，且速度全程不变
  const [spinAngle, setSpinAngle] = useState(0)
  const activity = useQueryActivity()
  const startedRef = useRef(false)
  const activityRef = useRef(0)
  const { notify } = useToast()
  activityRef.current = activity

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

  // 静默结束刷新态（不发提示）：没有可刷新的查询或兜底超时时使用
  const cancelRefresh = useCallback(() => {
    endRefreshWindow()
    startedRef.current = false
    setRefreshing(false)
  }, [])

  // 正常结束刷新态：发起过请求才提示，避免失败却报成功
  const finishRefresh = useCallback(() => {
    const failed = endRefreshWindow()
    const started = startedRef.current
    startedRef.current = false
    setRefreshing(false)
    if (started) {
      if (failed) notify('刷新失败，请稍后重试', 'error')
      else notify('数据已刷新')
    }
  }, [notify])

  // 刷新态收敛：等本次刷新触发的请求全部结束、静默 250ms 后再收尾
  useEffect(() => {
    if (!refreshing) return undefined
    if (activity > 0) {
      startedRef.current = true
      return undefined
    }
    // 当前页面没有可刷新的查询
    if (!startedRef.current) {
      const timer = window.setTimeout(cancelRefresh, 400)
      return () => window.clearTimeout(timer)
    }
    // 批次之间可能出现瞬时归零，静默期内仍为零才判定完成
    const timer = window.setTimeout(() => {
      if (activityRef.current === 0) finishRefresh()
    }, 250)
    return () => window.clearTimeout(timer)
  }, [activity, refreshing, cancelRefresh, finishRefresh])

  // 兜底：异常情况下最长转 15s（不提示，页面自身的 loading/error 仍在展示）
  useEffect(() => {
    if (!refreshing) return undefined
    const timer = window.setTimeout(cancelRefresh, 15000)
    return () => window.clearTimeout(timer)
  }, [refreshing, cancelRefresh])

  // 匀速旋转：立即起步，之后每 1s 走一整圈；停止时当前这一圈会走完并落在默认朝向
  useEffect(() => {
    if (!refreshing) return undefined
    setSpinAngle((angle) => angle + 360)
    const timer = window.setInterval(() => setSpinAngle((angle) => angle + 360), 1000)
    return () => window.clearInterval(timer)
  }, [refreshing])

  const refreshPage = () => {
    startedRef.current = false
    beginRefreshWindow()
    setRefreshing(true)
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
              disabled={refreshing}
              aria-busy={refreshing}
              className="w-8 h-8 shrink-0 flex items-center justify-center rounded-full text-gray-500 hover:text-gray-700 hover:bg-gray-100 disabled:cursor-default disabled:hover:bg-transparent dark:text-gray-400 dark:hover:text-gray-200 dark:hover:bg-gray-700/60 dark:disabled:hover:bg-transparent"
              aria-label={refreshing ? '正在刷新' : '刷新页面'}
              title={refreshing ? '正在刷新…' : '刷新当前页面数据'}
            >
              <svg
                className="w-4 h-4 transition-transform duration-1000 ease-linear"
                style={{ transform: `rotate(${spinAngle}deg)` }}
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <path d="M3 12a9 9 0 0 1 15.5-6.3L21 8" />
                <path d="M21 3v5h-5" />
                <path d="M21 12a9 9 0 0 1-15.5 6.3L3 16" />
                <path d="M3 21v-5h5" />
              </svg>
            </button>
            <TimeRangePicker className="min-w-0 flex-1 sm:flex-none" />
            <UsageFilterPicker />
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
