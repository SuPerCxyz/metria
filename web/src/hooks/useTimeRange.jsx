// 全局时间范围：跨页面保持，支持 URL 参数持久化。

import React, { createContext, useContext, useMemo, useState } from 'react'
import { quickRange } from '../services/format'

const TimeRangeContext = createContext(null)

const KEY = 'metria-range'

function readInitial() {
  try {
    const raw = sessionStorage.getItem(KEY)
    if (raw) {
      const p = JSON.parse(raw)
      if (p.from && p.to) return p
    }
  } catch { /* ignore */ }
  return quickRange('7d')
}

export function TimeRangeProvider({ children }) {
  const [range, setRange] = useState(readInitial)

  const set = (r) => {
    const next = { from: r.from, to: r.to, timezone: r.timezone || undefined }
    setRange(next)
    try { sessionStorage.setItem(KEY, JSON.stringify(next)) } catch { /* ignore */ }
  }

  const value = useMemo(() => ({ range, setRange: set }), [range])
  return <TimeRangeContext.Provider value={value}>{children}</TimeRangeContext.Provider>
}

export function useTimeRange() {
  const ctx = useContext(TimeRangeContext)
  if (!ctx) throw new Error('useTimeRange must be used within TimeRangeProvider')
  return ctx
}
