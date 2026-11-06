// Metria 应用：路由 + 鉴权守卫 + 分享公开页。

import React, { Suspense, lazy, useEffect } from 'react'
import { Routes, Route, Navigate, useLocation } from 'react-router-dom'
import './css/style.css'
import './charts/ChartjsConfig'

import AppLayout from './components/layout/AppLayout'
import ErrorBoundary from './components/common/ErrorBoundary'
import { TimeRangeProvider } from './hooks/useTimeRange'
import { NodeFilterProvider } from './hooks/useNodeFilter'
import { PageMetaProvider } from './hooks/usePageMeta'
import { ToastProvider } from './components/feedback/Toast'
import { AuthProvider, useAuth } from './hooks/useAuth'
import { getToken } from './services/api'

import Login from './pages/Login'
import OidcCallback from './pages/OidcCallback'
import Overview from './pages/overview/Overview'

// 低频页面按路由拆包；Overview 保留在首包，避免首屏多一次往返。
const Analytics = lazy(() => import('./pages/analytics/Analytics'))
const Sessions = lazy(() => import('./pages/sessions/Sessions'))
const SessionDetail = lazy(() => import('./pages/sessions/SessionDetail'))
const Calls = lazy(() => import('./pages/calls/Calls'))
const CallDetail = lazy(() => import('./pages/calls/CallDetail'))
const Nodes = lazy(() => import('./pages/nodes/Nodes'))
const NodeDetail = lazy(() => import('./pages/nodes/NodeDetail'))
const Agents = lazy(() => import('./pages/agents/Agents'))
const AgentDetail = lazy(() => import('./pages/agents/AgentDetail'))
const Models = lazy(() => import('./pages/models/Models'))
const ModelDetail = lazy(() => import('./pages/models/ModelDetail'))
const Costs = lazy(() => import('./pages/costs/Costs'))
const Settings = lazy(() => import('./pages/settings/Settings'))
const DataQuality = lazy(() => import('./pages/data-quality/DataQuality'))

function PageLoading() {
  return (
    <div className="flex min-h-[40vh] items-center justify-center bg-gray-100 dark:bg-gray-900">
      <div className="text-sm text-gray-400 dark:text-gray-500">加载中…</div>
    </div>
  )
}

function LazyPage({ children }) {
  return <Suspense fallback={<PageLoading />}>{children}</Suspense>
}

function RequireAuth({ children }) {
  const { authed } = useAuth()
  // 有本地 token 时先渲染布局，由 AuthProvider 的 /auth/me 在 401 时收回。
  if (!getToken() || authed === false) return <Navigate to="/login" replace />
  return children
}

function App() {
  const location = useLocation()

  useEffect(() => {
    window.scroll({ top: 0 })
  }, [location.pathname])

  return (
    <AuthProvider>
      <ToastProvider>
        <TimeRangeProvider>
        <NodeFilterProvider>
          <PageMetaProvider>
            <Routes>
              <Route path="/login" element={<Login />} />
              <Route path="/oidc-callback" element={<OidcCallback />} />
              <Route element={<RequireAuth><AppLayout /></RequireAuth>}>
                <Route path="/" element={<ErrorBoundary><Overview /></ErrorBoundary>} />
                <Route path="/analytics" element={<ErrorBoundary><LazyPage><Analytics /></LazyPage></ErrorBoundary>} />
                <Route path="/sessions" element={<ErrorBoundary><LazyPage><Sessions /></LazyPage></ErrorBoundary>} />
                <Route path="/sessions/:id" element={<ErrorBoundary><LazyPage><SessionDetail /></LazyPage></ErrorBoundary>} />
                <Route path="/calls" element={<ErrorBoundary><LazyPage><Calls /></LazyPage></ErrorBoundary>} />
                <Route path="/nodes" element={<ErrorBoundary><LazyPage><Nodes /></LazyPage></ErrorBoundary>} />
                <Route path="/nodes/:id" element={<ErrorBoundary><LazyPage><NodeDetail /></LazyPage></ErrorBoundary>} />
                <Route path="/agents" element={<ErrorBoundary><LazyPage><Agents /></LazyPage></ErrorBoundary>} />
                <Route path="/agents/:id" element={<ErrorBoundary><LazyPage><AgentDetail /></LazyPage></ErrorBoundary>} />
                <Route path="/models" element={<ErrorBoundary><LazyPage><Models /></LazyPage></ErrorBoundary>} />
                <Route path="/models/:id" element={<ErrorBoundary><LazyPage><ModelDetail /></LazyPage></ErrorBoundary>} />
                <Route path="/costs" element={<ErrorBoundary><LazyPage><Costs /></LazyPage></ErrorBoundary>} />
                <Route path="/calls/:id" element={<ErrorBoundary><LazyPage><CallDetail /></LazyPage></ErrorBoundary>} />
                <Route path="/data-quality" element={<ErrorBoundary><LazyPage><DataQuality /></LazyPage></ErrorBoundary>} />
                <Route path="/settings" element={<ErrorBoundary><LazyPage><Settings /></LazyPage></ErrorBoundary>} />
              </Route>
              <Route path="*" element={<Navigate to="/" replace />} />
            </Routes>
          </PageMetaProvider>
          </NodeFilterProvider>
        </TimeRangeProvider>
      </ToastProvider>
    </AuthProvider>
  )
}

export default App
