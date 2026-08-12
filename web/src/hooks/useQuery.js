// 轻量数据请求 hook：loading / error / data / refresh。

import { useCallback, useEffect, useRef, useState } from 'react'

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
    fetcherRef.current()
      .then((d) => {
        if (!cancelled) {
          setData(d)
          setLoading(false)
        }
      })
      .catch((e) => {
        if (!cancelled) {
          setError(e)
          setLoading(false)
        }
      })
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
