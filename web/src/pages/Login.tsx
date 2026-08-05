import { useState } from 'preact/hooks'
import { api, setToken } from '../api/client'
import { nav } from '../lib/router'

export function Login() {
  const [username, setUsername] = useState('admin')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  const submit = async (e: Event) => {
    e.preventDefault()
    setBusy(true)
    setError('')
    try {
      const res = await api<{ token: string }>('/auth/login', {
        method: 'POST',
        body: JSON.stringify({ username, password }),
      })
      setToken(res.token)
      window.dispatchEvent(new CustomEvent('metria:authed'))
      nav('overview')
    } catch (err) {
      setError(String((err as Error).message || err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div class="login-page">
      <form class="login-card" onSubmit={submit}>
        <h1>Metria</h1>
        <p class="login-sub">AI 编程 Agent 用量监控 · 费用分析 · 流量估算</p>
        <label>
          用户名
          <input value={username} onInput={(e) => setUsername((e.target as HTMLInputElement).value)} />
        </label>
        <label>
          密码
          <input type="password" value={password} onInput={(e) => setPassword((e.target as HTMLInputElement).value)} />
        </label>
        {error && <div class="login-error">{error}</div>}
        <button type="submit" class="btn primary" disabled={busy}>
          {busy ? '登录中…' : '登录'}
        </button>
      </form>
    </div>
  )
}
