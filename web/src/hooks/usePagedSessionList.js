// 会话明细分页加载：首屏固定 limit，按 next_cursor 追加，按 id 去重。

import { useCallback, useEffect, useRef, useState } from 'react'
import { api } from '../services/api'

export function usePagedSessionList(path, field, limit = 100) {
  const [items, setItems] = useState([])
  const [total, setTotal] = useState(null)
  const [hasMore, setHasMore] = useState(false)
  const [loading, setLoading] = useState(true)
  const [loadingMore, setLoadingMore] = useState(false)
  const [error, setError] = useState(null)
  const requestRef = useRef(0)
  const cursorRef = useRef(null)
  const hasMoreRef = useRef(false)
  const loadingRef = useRef(false)

  const fetchPage = useCallback(async (nextCursor, replace) => {
    const requestId = requestRef.current + 1
    requestRef.current = requestId
    loadingRef.current = true
    if (replace) setLoading(true)
    else setLoadingMore(true)
    try {
      const params = new URLSearchParams({ limit: String(limit) })
      if (nextCursor) params.set('cursor', nextCursor)
      const data = await api(`${path}?${params.toString()}`)
      if (requestRef.current !== requestId) return
      const page = Array.isArray(data?.[field]) ? data[field] : []
      setItems((previous) => {
        if (replace) return page
        const seen = new Set(previous.map((item) => item.id))
        return [...previous, ...page.filter((item) => !seen.has(item.id))]
      })
      setTotal(typeof data?.total === 'number' ? data.total : null)
      cursorRef.current = data?.next_cursor || null
      hasMoreRef.current = Boolean(data?.has_more)
      setHasMore(hasMoreRef.current)
      setError(null)
    } catch (e) {
      if (requestRef.current === requestId) setError(e)
    } finally {
      if (requestRef.current === requestId) {
        loadingRef.current = false
        setLoading(false)
        setLoadingMore(false)
      }
    }
  }, [field, limit, path])

  useEffect(() => {
    cursorRef.current = null
    hasMoreRef.current = false
    setItems([])
    setTotal(null)
    setHasMore(false)
    setError(null)
    fetchPage(null, true)
  }, [fetchPage])

  const loadMore = useCallback(() => {
    if (!hasMoreRef.current || loadingRef.current) return
    fetchPage(cursorRef.current, false)
  }, [fetchPage])

  const refresh = useCallback(() => {
    cursorRef.current = null
    hasMoreRef.current = false
    fetchPage(null, true)
  }, [fetchPage])

  // 顶栏刷新：重置游标并重新拉第一页；calls/timeline/tools 各自监听自己的路径。
  // 与 session-detail / session-stats 的 useQuery 刷新互不重复（端点不同）。
  useEffect(() => {
    const onRefresh = () => refresh()
    window.addEventListener('metria:refresh', onRefresh)
    return () => window.removeEventListener('metria:refresh', onRefresh)
  }, [refresh])

  return { items, total, hasMore, loading, loadingMore, error, loadMore, refresh }
}
