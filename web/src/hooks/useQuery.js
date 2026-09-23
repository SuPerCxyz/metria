// 轻量数据请求 hook：loading / error / data / refresh。

import { useCallback, useEffect, useRef, useState } from 'react'

// 全局在飞请求计数：供顶栏刷新按钮等展示「刷新中」。
let inFlight = 0
const activityListeners = new Set()

function bumpActivity(delta) {
  inFlight = Math.max(0, inFlight + delta)
  activityListeners.forEach((listener) => listener(inFlight))
}

// 同一查询键的在飞请求合并表：相同 key 的并发调用复用先发起的 Promise，只发一次网络请求。
const inFlightByKey = new Map()

/**
 * 合并同一查询键的在飞请求（导出供测试）。
 * fetcher 固定取请求发起时的那一份，不会被调用方后来的闭包覆盖；
 * 计数只统计真实发出的网络请求，复用方不再重复计数。
 */
export function sharedRequest(key, fetcher, { force = false } = {}) {
  // force=true（手动刷新）不复用在飞请求：刷新语义是「拿点击之后的最新数据」，
  // 复用点击前发出的 Promise 等于把过期快照当成本次刷新结果。
  const existing = force ? undefined : inFlightByKey.get(key)
  if (existing !== undefined) return existing
  const started = Promise.resolve(fetcher()) // fetcher 同步抛错直接向上传播
  bumpActivity(1)
  const promise = started.finally(() => {
    if (inFlightByKey.get(key) === promise) inFlightByKey.delete(key)
    bumpActivity(-1)
  })
  inFlightByKey.set(key, promise)
  return promise
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

  const run = useCallback((options) => {
    if (!enabledRef.current) {
      setLoading(false)
      return
    }
    const force = options?.force === true
    let cancelled = false
    setLoading(true)
    setError(null)
    sharedRequest(keyRef.current, fetcherRef.current, { force })
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
      // 事件发生时未启用的查询不在此响应刷新：其启用路径（如可见性激活）会发起，
      // 避免同一次刷新里 effect 与定时器各发一次请求。
      if (!enabledRef.current) return
      const keyAtRefresh = keyRef.current
      if (refreshTimerRef.current !== null) window.clearTimeout(refreshTimerRef.current)
      refreshTimerRef.current = window.setTimeout(() => {
        refreshTimerRef.current = null
        if (keyRef.current === keyAtRefresh) runRef.current?.({ force: true })
      }, 0)
    }
    window.addEventListener('metria:refresh', onRefresh)
    return () => {
      window.removeEventListener('metria:refresh', onRefresh)
      if (refreshTimerRef.current !== null) window.clearTimeout(refreshTimerRef.current)
    }
  }, [])

  // 组件主动调用的 refresh 等同手动刷新，同样穿透在飞去重。
  const refresh = useCallback(() => run({ force: true }), [run])
  return { data, loading, error, refresh }
}
