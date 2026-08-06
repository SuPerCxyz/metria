import { useEffect, useState } from 'preact/hooks'
import { api, getToken } from './api/client'
import { RangePicker } from './components/RangePicker'
import { LogoutButton } from './components/ui'
import { t, getLocale, setLocale } from './lib/i18n'
import { useRoute, nav } from './lib/router'
import { Login } from './pages/Login'
import { ShareView } from './pages/ShareView'
import { Overview } from './pages/Overview'
import { Nodes, NodeDetail } from './pages/Nodes'
import { Clients } from './pages/Clients'
import { ClientDetail } from './pages/ClientDetail'
import { Models, ModelDetail } from './pages/Models'
import { Sessions, SessionDetail } from './pages/Sessions'
import { Calls, CallDetail } from './pages/Calls'
import { Traffic } from './pages/Traffic'
import { TrafficProfiles } from './pages/TrafficProfiles'
import { DataQuality } from './pages/DataQuality'
import { Pricing } from './pages/Pricing'
import { Settings } from './pages/Settings'
import { Shares } from './pages/Shares'

const NAV = [
  ['overview', t('nav.overview')],
  ['nodes', t('nav.nodes')],
  ['clients', t('nav.clients')],
  ['models', t('nav.models')],
  ['sessions', t('nav.sessions')],
  ['calls', t('nav.calls')],
  ['traffic', t('nav.traffic')],
  ['traffic-profiles', t('nav.trafficProfiles')],
  ['pricing', t('nav.pricing')],
  ['data-quality', t('nav.dataQuality')],
  ['shares', t('nav.shares')],
  ['settings', t('nav.settings')],
] as const

function Page() {
  const route = useRoute()
  switch (route.path) {
    case '/overview':
      return <Overview />
    case '/nodes':
      return route.id ? <NodeDetail id={route.id} /> : <Nodes />
    case '/clients':
      return route.id ? <ClientDetail id={route.id} /> : <Clients />
    case '/models':
      return route.id ? <ModelDetail id={route.id} /> : <Models />
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
    const onUnauth = () => {
      setAuthed(false)
      nav('login')
    }
    const onAuthed = () => {
      // 登录成功后重新校验并进入主界面
      api('/auth/me')
        .then(() => setAuthed(true))
        .catch(() => {
          setAuthed(false)
          nav('login')
        })
    }
    window.addEventListener('metria:unauth', onUnauth)
    window.addEventListener('metria:authed', onAuthed)
    if (!getToken()) {
      setAuthed(false)
    } else {
      api('/auth/me')
        .then(() => setAuthed(true))
        .catch(() => {
          setAuthed(false)
          nav('login')
        })
    }
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
  const [localeTick, setLocaleTick] = useState(0)

  useEffect(() => {
    localStorage.setItem('metria-theme', theme)
  }, [theme])

  useEffect(() => {
    const onLocale = () => setLocaleTick((x) => x + 1)
    window.addEventListener('metria:locale', onLocale)
    return () => window.removeEventListener('metria:locale', onLocale)
  }, [])
  void localeTick

  if (authed === null) return <div class="state-box">{t('common.loading')}</div>
  if (route.path === '/s' && route.parts[1]) {
    return (
      <div data-theme={theme}>
        <ShareView slug={route.parts[1]} />
      </div>
    )
  }
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
              {theme === 'light' ? t('common.theme.dark') : t('common.theme.light')}
            </button>
            <button
              type="button"
              class="btn"
              onClick={() => setLocale(getLocale() === 'zh' ? 'en' : 'zh')}
            >
              {getLocale() === 'zh' ? 'EN' : '中文'}
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
