// Metria 侧边栏：总览 / 使用分析 / 会话 / 节点 / Agents / 模型 / 费用 / 网络流量 / 设置。

import React, { useEffect, useRef, useState } from 'react'
import { NavLink, useLocation } from 'react-router-dom'
import { cn } from '../lib/utils'

const NAV_ICONS = {
  overview: (
    <svg className="shrink-0 h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
      <path d="M3 13h4l2-7 4 12 2-5h6" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  ),
  analytics: (
    <svg className="shrink-0 h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
      <path d="M4 19V9m6 10V5m6 14v-7m6 7V3" strokeLinecap="round" />
    </svg>
  ),
  sessions: (
    <svg className="shrink-0 h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
      <path d="M8 10h8m-8 4h5m-2.5-8.5 4-2 1 4.5m-8.5-2.5-4 2 1 4.5" strokeLinecap="round" strokeLinejoin="round" />
      <rect x="3" y="5" width="18" height="14" rx="2" />
    </svg>
  ),
  nodes: (
    <svg className="shrink-0 h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
      <circle cx="12" cy="5" r="3" /><circle cx="5" cy="19" r="3" /><circle cx="19" cy="19" r="3" />
      <path d="M12 8v4m-7 4 4-2.5m10 2.5-4-2.5" strokeLinecap="round" />
    </svg>
  ),
  agents: (
    <svg className="shrink-0 h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M8 8h8m-8 4h8m-8 4h5" strokeLinecap="round" />
    </svg>
  ),
  models: (
    <svg className="shrink-0 h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
      <path d="M12 3v3m0 12v3m9-9h-3M6 12H3m14.5-6.5-2 2m-7 7-2 2m11 0-2-2m-7-7-2-2" strokeLinecap="round" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  ),
  costs: (
    <svg className="shrink-0 h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
      <path d="M12 3v18m3-15a3 3 0 0 0-6 0c0 4 9 3 9 7a3 3 0 0 1-6 0" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  ),
  traffic: (
    <svg className="shrink-0 h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
      <path d="M3 17h18M3 17l4-4m-4 4 4 4m14-8-4-4m4 4-4 4" strokeLinecap="round" strokeLinejoin="round" />
      <path d="M7 13v-2a2 2 0 0 1 2-2h6a2 2 0 0 1 2 2v2" strokeLinecap="round" />
    </svg>
  ),
  settings: (
    <svg className="shrink-0 h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  ),
}

const NAV = [
  { to: '/', label: '总览', icon: NAV_ICONS.overview, end: true },
  { to: '/analytics', label: '使用分析', icon: NAV_ICONS.analytics },
  { to: '/sessions', label: '会话', icon: NAV_ICONS.sessions },
  { to: '/nodes', label: '节点', icon: NAV_ICONS.nodes },
  { to: '/agents', label: 'Agents', icon: NAV_ICONS.agents },
  { to: '/models', label: '模型', icon: NAV_ICONS.models },
  { to: '/costs', label: '费用', icon: NAV_ICONS.costs },
  { to: '/traffic', label: '网络流量', icon: NAV_ICONS.traffic },
  { to: '/settings', label: '设置', icon: NAV_ICONS.settings },
]

function Sidebar({ sidebarOpen, setSidebarOpen }) {
  const location = useLocation()
  const { pathname } = location
  const trigger = useRef(null)
  const sidebar = useRef(null)
  const [expanded, setExpanded] = useState(() => localStorage.getItem('sidebar-expanded') === 'true')

  useEffect(() => {
    const stored = localStorage.getItem('sidebar-expanded')
    if (stored === 'true') document.body.classList.add('sidebar-expanded')
    else document.body.classList.remove('sidebar-expanded')
  }, [])

  useEffect(() => {
    const clickHandler = ({ target }) => {
      if (!sidebar.current || !trigger.current) return
      if (!sidebarOpen || sidebar.current.contains(target) || trigger.current.contains(target)) return
      setSidebarOpen(false)
    }
    document.addEventListener('click', clickHandler)
    return () => document.removeEventListener('click', clickHandler)
  }, [sidebarOpen])

  const closeSidebar = () => setSidebarOpen(false)

  return (
    <div className="min-w-fit sidebar-expanded:min-w-fit">
      <div ref={sidebar} className={`fixed inset-0 z-50 lg:static lg:ml-0 lg:translate-x-0 lg:w-64 xl:w-72 bg-white dark:bg-gray-900 border-r border-gray-200 dark:border-gray-700/60 shadow-sm transition-transform duration-300 ${sidebarOpen ? 'translate-x-0' : '-translate-x-full'}`}>
        <div className="flex flex-col h-full overflow-y-auto no-scrollbar">
          <div className="shrink-0 flex justify-between items-center px-6 h-16 border-b border-gray-200 dark:border-gray-700/60">
            <div className="flex items-center gap-2">
              <span className="w-8 h-8 rounded-lg bg-indigo-600 dark:bg-indigo-500 flex items-center justify-center text-white text-sm font-bold">M</span>
              <span className="text-lg font-bold text-gray-800 dark:text-gray-100 tracking-tight">Metria</span>
            </div>
            <button className="lg:hidden text-gray-500 hover:text-gray-600" onClick={closeSidebar} aria-label="关闭侧边栏">
              <svg className="w-5 h-5 fill-current" viewBox="0 0 24 24"><path d="M6.4 19 5 17.6l5.6-5.6L5 6.4 6.4 5l5.6 5.6L17.6 5 19 6.4 13.4 12l5.6 5.6-1.4 1.4-5.6-5.6L6.4 19Z" /></svg>
            </button>
          </div>

          <nav className="flex-1 px-4 py-6 space-y-1">
            {NAV.map((item) => {
              const active = item.end ? pathname === item.to : pathname.startsWith(item.to)
              return (
                <NavLink
                  key={item.to}
                  to={item.to}
                  end={item.end}
                  onClick={closeSidebar}
                  className={cn(
                    'flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors',
                    active
                      ? 'bg-indigo-50 dark:bg-indigo-500/10 text-indigo-600 dark:text-indigo-400'
                      : 'text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 hover:text-gray-800 dark:hover:text-gray-100'
                  )}
                >
                  {item.icon}
                  <span>{item.label}</span>
                </NavLink>
              )
            })}
          </nav>

          <div className="px-4 pb-6">
            <div className="rounded-xl border border-gray-200 dark:border-gray-700/60 p-3">
              <div className="text-xs text-gray-500 dark:text-gray-400 leading-relaxed">
                AI 编程 Agent 用量监控 · 费用分析 · 流量估算
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}

export default Sidebar
