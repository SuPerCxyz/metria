// Metria 侧边栏：总览 / 使用分析 / 会话 / 节点 / Agents / 模型 / 费用 / 网络流量 / 设置。

import React, { useEffect, useRef, useState } from 'react'
import { NavLink, useLocation } from 'react-router-dom'
import { cn } from '../lib/utils'

// Lucide 风格图标：统一尺寸、线宽和圆角，图标只承担语义提示。
const navIcon = (children) => (
  <svg
    className="shrink-0 h-5 w-5"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.75"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    {children}
  </svg>
)

const NAV_ICONS = {
  overview: navIcon(<>
    <rect width="7" height="9" x="3" y="3" rx="1" />
    <rect width="7" height="5" x="14" y="3" rx="1" />
    <rect width="7" height="9" x="14" y="12" rx="1" />
    <rect width="7" height="5" x="3" y="16" rx="1" />
  </>),
  analytics: navIcon(<>
    <path d="M3 3v18h18" />
    <path d="m7 16 4-5 4 3 5-7" />
  </>),
  sessions: navIcon(<>
    <path d="M16 10a2 2 0 0 1-2 2H6.828a2 2 0 0 0-1.414.586l-2.202 2.202A.71.71 0 0 1 2 14.286V4a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z" />
    <path d="M20 9a2 2 0 0 1 2 2v10.286a.71.71 0 0 1-1.212.502l-2.202-2.202A2 2 0 0 0 17.172 19H10a2 2 0 0 1-2-2v-1" />
  </>),
  nodes: navIcon(<>
    <rect width="16" height="7" x="4" y="3" rx="1.5" />
    <rect width="16" height="7" x="4" y="14" rx="1.5" />
    <path d="M8 6.5h.01M12 6.5h.01M8 17.5h.01M12 17.5h.01M12 10v4" />
  </>),
  agents: navIcon(<>
    <path d="M12 8V4H8" />
    <rect width="16" height="12" x="4" y="8" rx="2" />
    <path d="M2 14h2M20 14h2M15 13v2M9 13v2" />
  </>),
  models: navIcon(<>
    <rect width="16" height="16" x="4" y="4" rx="2" />
    <rect width="6" height="6" x="9" y="9" rx="1" />
    <path d="M9 1v3M15 1v3M9 20v3M15 20v3M20 9h3M20 14h3M1 9h3M1 14h3" />
  </>),
  costs: navIcon(<>
    <path d="M4 2v20l2-1 2 1 2-1 2 1 2-1 2 1 2-1 2 1V2l-2 1-2-1-2 1-2-1-2 1-2-1-2 1Z" />
    <path d="M16 8h-6M16 12h-6M13 16h-3" />
  </>),
  traffic: navIcon(<>
    <path d="m3 16 4 4 4-4M7 20V4" />
    <path d="m21 8-4-4-4 4M17 4v16" />
  </>),
  settings: navIcon(<>
    <path d="M20 7h-9M14 17H5" />
    <circle cx="17" cy="17" r="3" />
    <circle cx="7" cy="7" r="3" />
  </>),
}

const NAV_GROUPS = [
  {
    label: '监控',
    items: [
      { to: '/', label: '总览', icon: NAV_ICONS.overview, end: true },
      { to: '/analytics', label: '使用分析', icon: NAV_ICONS.analytics },
      { to: '/sessions', label: '会话', icon: NAV_ICONS.sessions },
      { to: '/nodes', label: '节点', icon: NAV_ICONS.nodes },
    ],
  },
  {
    label: '资源',
    items: [
      { to: '/agents', label: 'Agents', icon: NAV_ICONS.agents },
      { to: '/models', label: '模型', icon: NAV_ICONS.models },
    ],
  },
  {
    label: '分析',
    items: [
      { to: '/costs', label: '费用', icon: NAV_ICONS.costs },
      { to: '/traffic', label: '网络流量', icon: NAV_ICONS.traffic },
    ],
  },
  {
    label: '系统',
    items: [{ to: '/settings', label: '设置', icon: NAV_ICONS.settings }],
  },
]

function Sidebar({ sidebarOpen, setSidebarOpen }) {
  const location = useLocation()
  const { pathname } = location
  const trigger = useRef(null)
  const sidebar = useRef(null)
  const [expanded, setExpanded] = useState(() => {
    try { return localStorage.getItem('sidebar-expanded') === 'true' } catch { return false }
  })

  useEffect(() => {
    if (expanded) document.body.classList.add('sidebar-expanded')
    else document.body.classList.remove('sidebar-expanded')
    try { localStorage.setItem('sidebar-expanded', String(expanded)) } catch { /* ignore */ }
  }, [expanded])

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
    <div className={`${expanded ? 'lg:w-[226px] xl:w-[226px]' : 'lg:w-20 xl:w-20'} transition-all duration-300`}>
      <div ref={sidebar} className={`fixed inset-0 z-50 lg:static lg:ml-0 lg:translate-x-0 bg-white dark:bg-gray-900 border-r border-gray-200 dark:border-gray-700/60 shadow-sm transition-transform duration-300 ${sidebarOpen ? 'translate-x-0' : '-translate-x-full'}`}>
        <div className="flex flex-col h-full overflow-y-auto no-scrollbar">
          <div className="shrink-0 flex items-center px-6 h-16 border-b border-gray-200 dark:border-gray-700/60">
            <div className="flex items-center gap-3">
              <img src="/static/metria-logo.png" alt="Metria" className="w-10 h-10 shrink-0 rounded-lg object-cover" />
              <span className={`${expanded ? '' : 'lg:hidden'} mt-[5px] text-3xl font-bold text-gray-800 dark:text-gray-100 tracking-widest leading-none whitespace-nowrap`}>
                Metria
              </span>
            </div>
            <button className="ml-auto lg:hidden text-gray-500 hover:text-gray-600" onClick={closeSidebar} aria-label="关闭侧边栏">
              <svg className="w-5 h-5 fill-current" viewBox="0 0 24 24"><path d="M6.4 19 5 17.6l5.6-5.6L5 6.4 6.4 5l5.6 5.6L17.6 5 19 6.4 13.4 12l5.6 5.6-1.4 1.4-5.6-5.6L6.4 19Z" /></svg>
            </button>
          </div>

          <nav className="flex-1 px-4 py-6">
            {NAV_GROUPS.map((group, groupIndex) => (
              <div key={group.label} className={groupIndex === 0 ? '' : 'mt-5'}>
                {expanded && (
                  <div className="px-3 mb-2 text-[10px] font-semibold uppercase tracking-[0.16em] text-gray-400 dark:text-gray-500">
                    {group.label}
                  </div>
                )}
                <div className="space-y-1">
                  {group.items.map((item) => {
                    const active = item.end ? pathname === item.to : pathname.startsWith(item.to)
                    return (
                      <NavLink
                        key={item.to}
                        to={item.to}
                        end={item.end}
                        onClick={closeSidebar}
                        title={expanded ? undefined : item.label}
                        className={cn(
                          'flex items-center gap-3 px-3 py-2.5 rounded-lg text-base font-medium transition-colors',
                          active
                            ? 'bg-indigo-50 dark:bg-indigo-500/10 text-indigo-600 dark:text-indigo-400'
                            : 'text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 hover:text-gray-800 dark:hover:text-gray-100'
                        )}
                      >
                        {item.icon}
                        <span className={`${expanded ? '' : 'lg:hidden'} whitespace-nowrap`}>{item.label}</span>
                      </NavLink>
                    )
                  })}
                </div>
              </div>
            ))}
          </nav>

          <div className="px-4 pb-6 space-y-1">
            <button
              type="button"
              onClick={() => setExpanded(!expanded)}
              aria-label={expanded ? '收起侧边栏' : '展开侧边栏'}
              title={expanded ? '收起侧边栏' : '展开侧边栏'}
              className="w-full hidden lg:flex items-center justify-start gap-3 px-3 py-2.5 rounded-lg text-base font-medium text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 hover:text-gray-800 dark:hover:text-gray-100 transition-colors"
            >
              <svg className="shrink-0 h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                {expanded
                  ? <path d="m15 18-6-6 6-6" strokeLinecap="round" strokeLinejoin="round" />
                  : <path d="m9 18 6-6-6-6" strokeLinecap="round" strokeLinejoin="round" />}
              </svg>
              {expanded && <span className="whitespace-nowrap">{expanded ? '收起侧边栏' : '展开侧边栏'}</span>}
            </button>
            {expanded && (
              <div className="rounded-xl border border-gray-200 dark:border-gray-700/60 p-3">
                <div className="text-xs text-gray-500 dark:text-gray-400 leading-relaxed">
                  AI 编程 Agent 用量监控 · 费用分析 · 流量估算
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}

export default Sidebar
