// 全局鉴权状态：应用顶层只请求一次 /auth/me，Header/守卫/设置页共享。

import React, { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import { api, getToken } from '../services/api'
import { clearQueryCache } from './useQuery'

const AuthContext = createContext(null)

export function AuthProvider({ children }) {
  const [user, setUser] = useState(null)
  const [authed, setAuthed] = useState(() => (getToken() ? null : false))

  const load = useCallback(() => {
    if (!getToken()) {
      setUser(null)
      setAuthed(false)
      return Promise.resolve(null)
    }
    setAuthed(null)
    return api('/auth/me')
      .then((profile) => {
        setUser(profile)
        setAuthed(true)
        return profile
      })
      .catch(() => {
        setUser(null)
        setAuthed(false)
        return null
      })
  }, [])

  useEffect(() => {
    load()
    const onUnauth = () => {
      clearQueryCache()
      setUser(null)
      setAuthed(false)
    }
    const onAuthed = () => { load() }
    const onProfileUpdated = () => { load() }
    window.addEventListener('metria:unauth', onUnauth)
    window.addEventListener('metria:authed', onAuthed)
    window.addEventListener('metria:profile-updated', onProfileUpdated)
    return () => {
      window.removeEventListener('metria:unauth', onUnauth)
      window.removeEventListener('metria:authed', onAuthed)
      window.removeEventListener('metria:profile-updated', onProfileUpdated)
    }
  }, [load])

  const value = useMemo(
    () => ({ user, authed, refreshUser: load }),
    [user, authed, load]
  )
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth() {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error('useAuth must be used within AuthProvider')
  return ctx
}
