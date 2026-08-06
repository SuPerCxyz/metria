// Metria 应用：路由 + 鉴权守卫 + 分享公开页。

import React, { useEffect, useState } from 'react'
import { Routes, Route, Navigate, useLocation } from 'react-router-dom'
import './css/style.css'
import './charts/ChartjsConfig'

import AppLayout from './components/layout/AppLayout'
import ErrorBoundary from './components/common/ErrorBoundary'
import { TimeRangeProvider } from './hooks/useTimeRange'
import { getToken, api } from './services/api'

import Login from './pages/Login'
import Overview from './pages/overview/Overview'
import Analytics from './pages/analytics/Analytics'
import Sessions from './pages/sessions/Sessions'
import SessionDetail from './pages/sessions/SessionDetail'
import Nodes from './pages/nodes/Nodes'
import NodeDetail from './pages/nodes/NodeDetail'
import Agents from './pages/agents/Agents'
import AgentDetail from './pages/agents/AgentDetail'
import Models from './pages/models/Models'
import ModelDetail from './pages/models/ModelDetail'
import Costs from './pages/costs/Costs'
import Traffic from './pages/traffic/Traffic'
import Settings from './pages/settings/Settings'
import CallDetail from './pages/calls/CallDetail'

function RequireAuth({ children }) {
  const [authed, setAuthed] = useState(null)

  useEffect(() => {
    const check = () => {
      if (!getToken()) {
        setAuthed(false)
        return
      }
      api('/auth/me')
        .then(() => setAuthed(true))
        .catch(() => setAuthed(false))
    }
    const onUnauth = () => setAuthed(false)
    const onAuthed = () => {
      api('/auth/me').then(() => setAuthed(true)).catch(() => setAuthed(false))
    }
    check()
    window.addEventListener('metria:unauth', onUnauth)
    window.addEventListener('metria:authed', onAuthed)
    return () => {
      window.removeEventListener('metria:unauth', onUnauth)
      window.removeEventListener('metria:authed', onAuthed)
    }
  }, [])

  if (authed === null) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-100 dark:bg-gray-900">
        <div className="text-sm text-gray-400 dark:text-gray-500">加载中…</div>
      </div>
    )
  }
  if (!authed) return <Navigate to="/login" replace />
  return children
}

function App() {
  const location = useLocation()

  useEffect(() => {
    window.scroll({ top: 0 })
  }, [location.pathname])

  return (
    <TimeRangeProvider>
      <Routes>
        <Route path="/login" element={<Login />} />
        <Route element={<RequireAuth><AppLayout /></RequireAuth>}>
          <Route path="/" element={<ErrorBoundary><Overview /></ErrorBoundary>} />
          <Route path="/analytics" element={<ErrorBoundary><Analytics /></ErrorBoundary>} />
          <Route path="/sessions" element={<ErrorBoundary><Sessions /></ErrorBoundary>} />
          <Route path="/sessions/:id" element={<ErrorBoundary><SessionDetail /></ErrorBoundary>} />
          <Route path="/nodes" element={<ErrorBoundary><Nodes /></ErrorBoundary>} />
          <Route path="/nodes/:id" element={<ErrorBoundary><NodeDetail /></ErrorBoundary>} />
          <Route path="/agents" element={<ErrorBoundary><Agents /></ErrorBoundary>} />
          <Route path="/agents/:id" element={<ErrorBoundary><AgentDetail /></ErrorBoundary>} />
          <Route path="/models" element={<ErrorBoundary><Models /></ErrorBoundary>} />
          <Route path="/models/:id" element={<ErrorBoundary><ModelDetail /></ErrorBoundary>} />
          <Route path="/costs" element={<ErrorBoundary><Costs /></ErrorBoundary>} />
          <Route path="/traffic" element={<ErrorBoundary><Traffic /></ErrorBoundary>} />
          <Route path="/calls/:id" element={<ErrorBoundary><CallDetail /></ErrorBoundary>} />
          <Route path="/settings" element={<ErrorBoundary><Settings /></ErrorBoundary>} />
        </Route>
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </TimeRangeProvider>
  )
}

export default App
