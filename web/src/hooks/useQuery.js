// 轻量数据请求 hook：loading / error / data / refresh。

import { useCallback, useEffect, useRef, useState } from 'react'

export function useQuery(key, fetcher, { enabled = true } = {}) {
  const [data, setData] = useState(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const keyRef = useRef(key)
  const enabledRef = useRef(enabled)

  const run = useCallback(() => {
    if (!enabledRef.current) {
      setLoading(false)
      return
    }
    let cancelled = false
    setLoading(true)
    setError(null)
    fetcher()
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

  useEffect(() => {
    const cleanup = run()
    return cleanup
  }, [key, enabled])

  const refresh = useCallback(() => run(), [run])
  return { data, loading, error, refresh }
}
