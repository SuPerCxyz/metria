// 登录页。

import React, { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { api, setToken } from '../services/api'

export default function Login() {
  const [username, setUsername] = useState('admin')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const navigate = useNavigate()

  const submit = async (e) => {
    e.preventDefault()
    setBusy(true)
    setError('')
    try {
      const res = await api('/auth/login', {
        method: 'POST',
        body: JSON.stringify({ username, password }),
      })
      setToken(res.token)
      window.dispatchEvent(new CustomEvent('metria:authed'))
      navigate('/')
    } catch (err) {
      setError(err?.message || '登录失败')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-gray-100 dark:bg-gray-900 px-4">
      <div className="w-full max-w-sm">
        <div className="mb-8 text-center">
          <span className="inline-flex w-12 h-12 rounded-xl bg-indigo-600 dark:bg-indigo-500 items-center justify-center text-white text-xl font-bold mb-4">M</span>
          <h1 className="text-2xl font-bold text-gray-800 dark:text-gray-100">Metria</h1>
          <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">AI 编程 Agent 用量监控 · 费用分析 · 流量估算</p>
        </div>
        <form onSubmit={submit} className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <div className="space-y-4">
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">用户名</label>
              <input
                type="text"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-700 text-gray-800 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500/30 focus:border-indigo-500"
              />
            </div>
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">密码</label>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-700 text-gray-800 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500/30 focus:border-indigo-500"
              />
            </div>
            {error && <div className="text-sm text-red-600 dark:text-red-400">{error}</div>}
            <button
              type="submit"
              disabled={busy}
              className="w-full py-2.5 rounded-lg bg-indigo-600 hover:bg-indigo-700 text-white text-sm font-medium disabled:opacity-50"
            >
              {busy ? '登录中…' : '登录'}
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}
