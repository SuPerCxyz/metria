// 轻量数据请求 hook：loading / error / data / refresh。

import { useCallback, useEffect, useRef, useState } from 'react'

// 全局在飞请求计数：供顶栏刷新按钮等展示「刷新中」。
let inFlight = 0
const activityListeners = new Set()

function bumpActivity(delta) {
  inFlight = Math.max(0, inFlight + delta)
  activityListeners.forEach((listener) => listener(inFlight))
}

// 刷新窗口：记录「本次刷新触发的请求是否有失败」，供顶栏提示成功/失败。
let refreshPending = false
let refreshFailed = false

export function beginRefreshWindow() {
  refreshPending = true
  refreshFailed = false
}

/** 供内部与测试标记本次刷新中出现过失败请求。 */
export function markRefreshFailure() {
  if (refreshPending) refreshFailed = true
}

/** 结束刷新窗口，返回本次刷新是否出现过失败。 */
export function endRefreshWindow() {
  const failed = refreshFailed
  refreshPending = false
  refreshFailed = false
  return failed
}

/** 订阅当前在飞请求数（任何页面任一 useQuery 发起的请求）。 */
export function useQueryActivity() {
  const [count, setCount] = useState(inFlight)
  useEffect(() => {
    activityListeners.add(setCount)
    setCount(inFlight)
    return () => { activityListeners.delete(setCount) }
  }, [])
  return count
}

export function useQuery(key, fetcher, { enabled = true } = {}) {
  const [data, setData] = useState(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const keyRef = useRef(key)
  const enabledRef = useRef(enabled)
  const fetcherRef = useRef(fetcher)
  const runRef = useRef(null)
  const refreshTimerRef = useRef(null)

  keyRef.current = key
  enabledRef.current = enabled
  fetcherRef.current = fetcher

  const run = useCallback(() => {
    if (!enabledRef.current) {
      setLoading(false)
      return
    }
    let cancelled = false
    setLoading(true)
    setError(null)
    bumpActivity(1)
    fetcherRef.current()
      .then((d) => {
        if (!cancelled) {
          setData(d)
          setLoading(false)
        }
      })
      .catch((e) => {
        markRefreshFailure()
        if (!cancelled) {
          setError(e)
          setLoading(false)
        }
      })
      .finally(() => bumpActivity(-1))
    return () => { cancelled = true }
  }, [key]) // eslint-disable-line react-hooks/exhaustive-deps

  runRef.current = run

  useEffect(() => {
    const cleanup = run()
    return cleanup
  }, [key, enabled])

  // 等时间范围状态落稳后再刷新；范围 key 已变化的查询由上面的 effect 执行，避免重复请求。
  useEffect(() => {
    const onRefresh = () => {
      const keyAtRefresh = keyRef.current
      if (refreshTimerRef.current !== null) window.clearTimeout(refreshTimerRef.current)
      refreshTimerRef.current = window.setTimeout(() => {
        refreshTimerRef.current = null
        if (keyRef.current === keyAtRefresh) runRef.current?.()
      }, 0)
    }
    window.addEventListener('metria:refresh', onRefresh)
    return () => {
      window.removeEventListener('metria:refresh', onRefresh)
      if (refreshTimerRef.current !== null) window.clearTimeout(refreshTimerRef.current)
    }
  }, [])

  const refresh = useCallback(() => run(), [run])
  return { data, loading, error, refresh }
}
