// 全局时间范围：打开页面时固定为「今天凌晨 → 当前时间」，不沿用上次保存的范围。

import React, { createContext, useCallback, useContext, useMemo, useState } from 'react'
import {
  createPresetRange,
  DEFAULT_PRESET_KEY,
  isSameTimeRange,
  normalizeTimeRange,
  refreshPresetRange,
} from './timeRangeState'

const TimeRangeContext = createContext(null)

function readInitial() {
  return createPresetRange(DEFAULT_PRESET_KEY)
}

export function TimeRangeProvider({ children }) {
  const [range, setRange] = useState(readInitial)

  const set = useCallback((next) => setRange(normalizeTimeRange(next)), [])

  const refreshRange = useCallback(() => {
    const next = refreshPresetRange(range)
    if (!next || isSameTimeRange(range, next)) return false
    setRange(next)
    return true
  }, [range])

  const value = useMemo(() => ({ range, setRange: set, refreshRange }), [range, set, refreshRange])
  return <TimeRangeContext.Provider value={value}>{children}</TimeRangeContext.Provider>
}

export function useTimeRange() {
  const ctx = useContext(TimeRangeContext)
  if (!ctx) throw new Error('useTimeRange must be used within TimeRangeProvider')
  return ctx
}
