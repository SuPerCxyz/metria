import { useEffect, useState } from 'preact/hooks'
import { api, getToken } from './api/client'
import { RangePicker } from './components/RangePicker'
import { LogoutButton } from './components/ui'
import { useRoute, nav } from './lib/router'
import { Login } from './pages/Login'
import { Overview } from './pages/Overview'
import { Nodes, NodeDetail } from './pages/Nodes'
import { Clients } from './pages/Clients'
import { Models } from './pages/Models'
import { Sessions, SessionDetail } from './pages/Sessions'
import { Calls, CallDetail } from './pages/Calls'
import { Traffic } from './pages/Traffic'
import { TrafficProfiles } from './pages/TrafficProfiles'
import { DataQuality } from './pages/DataQuality'
import { Pricing } from './pages/Pricing'
import { Settings } from './pages/Settings'
import { Shares } from './pages/Shares'

const NAV = [
  ['overview', '总览'],
  ['nodes', 'Nodes'],
  ['clients', 'Agent 工具'],
  ['models', '模型'],
  ['sessions', '会话'],
  ['calls', '调用'],
  ['traffic', '流量'],
  ['traffic-profiles', 'Traffic Profiles'],
  ['pricing', '价格'],
  ['data-quality', '数据质量'],
  ['shares', '分享'],
  ['settings', '设置'],
] as const

function Page() {
  const route = useRoute()
  switch (route.path) {
    case '/overview':
      return <Overview />
    case '/nodes':
      return route.id ? <NodeDetail id={route.id} /> : <Nodes />
    case '/clients':
      return <Clients />
    case '/models':
      return <Models />
    case '/sessions':
      return route.id ? <SessionDetail id={route.id} /> : <Sessions />
    case '/calls':
      return route.id ? <CallDetail id={route.id} /> : <Calls />
    case '/traffic':
      return <Traffic />
    case '/traffic-profiles':
      return <TrafficProfiles />
    case '/pricing':
      return <Pricing />
    case '/shares':
      return <Shares />
    case '/settings':
      return <Settings />
    case '/data-quality':
      return <DataQuality />
    default:
      return <Overview />
  }
}

export function App() {
  const route = useRoute()
  const [authed, setAuthed] = useState<boolean | null>(null)

  useEffect(() => {
    if (!getToken()) {
      setAuthed(false)
      return
    }
    api('/auth/me')
      .then(() => setAuthed(true))
      .catch(() => {
        setAuthed(false)
        nav('login')
      })
    const onUnauth = () => {
      setAuthed(false)
      nav('login')
    }
    const onAuthed = () => setAuthed(true)
    window.addEventListener('metria:unauth', onUnauth)
    window.addEventListener('metria:authed', onAuthed)
    return () => {
      window.removeEventListener('metria:unauth', onUnauth)
      window.removeEventListener('metria:authed', onAuthed)
    }
  }, [])

  const [theme, setTheme] = useState<'light' | 'dark'>(() => {
    const s = localStorage.getItem('metria-theme')
    if (s === 'light' || s === 'dark') return s
    return window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
  })

  useEffect(() => {
    localStorage.setItem('metria-theme', theme)
  }, [theme])

  if (authed === null) return <div class="state-box">加载中…</div>
  if (!authed) {
    return (
      <div data-theme={theme}>
        <Login />
      </div>
    )
  }
  if (route.path === '/login') {
    nav('overview')
  }

  return (
    <div data-theme={theme} class="app-shell">
      <aside class="sidebar">
        <div class="sidebar-title">Metria</div>
        <nav class="sidebar-nav">
          {NAV.map(([p, label]) => (
            <a key={p} class={route.path === `/${p}` ? 'active' : ''} onClick={() => nav(p)} href={`#/${p}`}>
              {label}
            </a>
          ))}
        </nav>
      </aside>
      <div class="main">
        <header class="topbar">
          <RangePicker />
          <div class="topbar-right">
            <button
              type="button"
              class="btn"
              onClick={() => setTheme(theme === 'light' ? 'dark' : 'light')}
            >
              {theme === 'light' ? '暗色' : '亮色'}
            </button>
            <LogoutButton />
          </div>
        </header>
        <main class="content">
          <Page />
        </main>
      </div>
    </div>
  )
}
