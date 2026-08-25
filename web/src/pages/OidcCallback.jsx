// OIDC 回调落地页：用一次性交换码换取会话 token。

import React, { useEffect, useState } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router-dom'
import { api, setToken } from '../services/api'

export default function OidcCallback() {
  const [params] = useSearchParams()
  const navigate = useNavigate()
  const [error, setError] = useState('')

  useEffect(() => {
    const code = params.get('code')
    if (!code) {
      setError('缺少授权码，请从登录页重新发起 OIDC 登录')
      return
    }
    api('/auth/oidc/exchange', {
      method: 'POST',
      body: JSON.stringify({ code }),
    })
      .then((res) => {
        setToken(res.token)
        window.dispatchEvent(new CustomEvent('metria:authed'))
        navigate('/', { replace: true })
      })
      .catch((err) => setError(err?.message || 'OIDC 登录失败'))
    // 仅在挂载时执行一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  return (
    <div className="min-h-screen flex items-center justify-center bg-gray-100 dark:bg-gray-900 px-4">
      <div className="w-full max-w-sm text-center">
        {error ? (
          <>
            <h1 className="text-lg font-semibold text-gray-800 dark:text-gray-100 mb-2">OIDC 登录失败</h1>
            <p className="text-sm text-red-600 dark:text-red-400 mb-6">{error}</p>
            <Link
              to="/login"
              className="inline-block px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-700 text-white text-sm font-medium"
            >
              返回登录页
            </Link>
          </>
        ) : (
          <p className="text-sm text-gray-500 dark:text-gray-400">正在完成 OIDC 登录…</p>
        )}
      </div>
    </div>
  )
}
