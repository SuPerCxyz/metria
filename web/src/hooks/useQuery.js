// 轻量数据请求 hook：loading / error / data / refresh + SWR 缓存。

import { useCallback, useEffect, useRef, useState } from 'react'
import { withAbortSignal } from '../services/api.js'

/** SWR 缓存有效期（15 秒）。 */
export const QUERY_CACHE_TTL_MS = 15_000
const QUERY_CACHE_MAX = 200
const QUERY_DEBOUNCE_MS = 120

const queryCache = new Map()

/** 读取缓存条目；不存在返回 null。 */
export function getQueryCache(key) {
  return queryCache.get(key) || null
}

/** 写入缓存；超过上限时淘汰最旧条目。 */
export function putQueryCache(key, data, now = Date.now()) {
  queryCache.set(key, { data, updatedAt: now })
  if (queryCache.size > QUERY_CACHE_MAX) {
    queryCache.delete(queryCache.keys().next().value)
  }
}

export function clearQueryCache() {
  queryCache.clear()
}

export function isQueryCacheFresh(entry, now = Date.now()) {
  return Boolean(entry) && now - entry.updatedAt < QUERY_CACHE_TTL_MS
}

// 全局在飞请求计数：供顶栏刷新按钮等展示「刷新中」。
let inFlight = 0
const activityListeners = new Set()

function bumpActivity(delta) {
  inFlight = Math.max(0, inFlight + delta)
  activityListeners.forEach((listener) => listener(inFlight))
}

// 同一查询键的在飞请求合并表：相同 key 的并发调用复用同一个底层请求。
// entry: { promise, controller, waiters:Set<symbol>, settled:boolean }
const inFlightByKey = new Map()

function abortError() {
  if (typeof DOMException !== 'undefined') {
    return new DOMException('Aborted', 'AbortError')
  }
  const error = new Error('Aborted')
  error.name = 'AbortError'
  return error
}

/**
 * 合并同一查询键的在飞请求（导出供测试）。
 *
 * 底层请求绑定独立的 AbortController，不绑定任何单个调用者的 signal：
 * 一个组件卸载只取消它自己的等待；只有所有等待者都取消后才 abort 底层请求。
 */
export function sharedRequest(key, fetcher, { force = false, signal } = {}) {
  if (signal?.aborted) return Promise.reject(abortError())
  // force=true（手动刷新）不复用在飞请求：刷新语义是「拿点击之后的最新数据」。
  let existing = force ? undefined : inFlightByKey.get(key)
  if (existing && (existing.settled || existing.controller.signal.aborted)) {
    existing = undefined
  }
  const entry = existing || createSharedEntry(key, fetcher)
  return subscribeShared(entry, signal)
}

function createSharedEntry(key, fetcher) {
  const controller = new AbortController()
  const entry = {
    controller,
    waiters: new Set(),
    settled: false,
    promise: null,
  }
  // fetcher 同步调用 api；withAbortSignal 让 api 的 fetch 拿到底层请求的 signal。
  const started = Promise.resolve(
    withAbortSignal(controller.signal, () => fetcher({ signal: controller.signal })),
  )
  bumpActivity(1)
  entry.promise = started.finally(() => {
    entry.settled = true
    if (inFlightByKey.get(key) === entry) inFlightByKey.delete(key)
    bumpActivity(-1)
  })
  inFlightByKey.set(key, entry)
  return entry
}

function subscribeShared(entry, signal) {
  const token = Symbol('shared-request-waiter')
  entry.waiters.add(token)

  return new Promise((resolve, reject) => {
    let settled = false
    const cleanup = () => {
      if (signal) signal.removeEventListener('abort', onAbort)
      entry.waiters.delete(token)
    }
    const finish = (fn, value) => {
      if (settled) return
      settled = true
      cleanup()
      fn(value)
    }
    const onAbort = () => {
      finish(reject, abortError())
      // 最后一个等待者取消时才取消底层请求；其他复用者仍可拿到结果。
      if (entry.waiters.size === 0 && !entry.settled) entry.controller.abort()
    }
    if (signal) signal.addEventListener('abort', onAbort, { once: true })
    entry.promise.then(
      (value) => finish(resolve, value),
      (error) => finish(reject, error),
    )
  })
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

export function useQuery(key, fetcher, { enabled = true, debounceMs = QUERY_DEBOUNCE_MS } = {}) {
  const initial = getQueryCache(key)
  const [data, setData] = useState(initial?.data ?? null)
  const [loading, setLoading] = useState(!initial)
  const [error, setError] = useState(null)
  const keyRef = useRef(key)
  const enabledRef = useRef(enabled)
  const fetcherRef = useRef(fetcher)
  const runRef = useRef(null)
  const refreshTimerRef = useRef(null)
  const controllerRef = useRef(null)
  const debounceTimerRef = useRef(null)
  const firstRunRef = useRef(true)

  keyRef.current = key
  enabledRef.current = enabled
  fetcherRef.current = fetcher

  const run = useCallback((options) => {
    if (!enabledRef.current) {
      setLoading(false)
      return
    }
    const force = options?.force === true
    if (debounceTimerRef.current !== null) {
      window.clearTimeout(debounceTimerRef.current)
      debounceTimerRef.current = null
    }
    controllerRef.current?.abort()
    const controller = new AbortController()
    controllerRef.current = controller
    const keyAtRun = keyRef.current

    if (!force) {
      const cached = getQueryCache(keyAtRun)
      if (cached) {
        setData(cached.data)
        setLoading(!isQueryCacheFresh(cached))
      } else {
        setLoading(true)
      }
    } else {
      setLoading(true)
    }
    setError(null)

    sharedRequest(keyAtRun, fetcherRef.current, { force, signal: controller.signal })
      .then((d) => {
        if (controller.signal.aborted || keyRef.current !== keyAtRun) return
        putQueryCache(keyAtRun, d)
        setData(d)
        setLoading(false)
      })
      .catch((e) => {
        if (controller.signal.aborted || keyRef.current !== keyAtRun) return
        markRefreshFailure()
        setError(e)
        setLoading(false)
      })
  }, [key]) // eslint-disable-line react-hooks/exhaustive-deps

  runRef.current = run

  useEffect(() => {
    const cached = getQueryCache(key)
    setData(cached?.data ?? null)
    setError(null)
    if (!enabled) {
      setLoading(false)
      return undefined
    }
    setLoading(!cached || !isQueryCacheFresh(cached))
    const delay = firstRunRef.current ? 0 : Math.max(0, debounceMs)
    firstRunRef.current = false
    const timer = window.setTimeout(() => {
      debounceTimerRef.current = null
      run()
    }, delay)
    debounceTimerRef.current = timer
    return () => {
      window.clearTimeout(timer)
      debounceTimerRef.current = null
      controllerRef.current?.abort()
    }
  }, [key, enabled, debounceMs, run])

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

  // 组件主动调用的 refresh 等同手动刷新，同样穿透缓存与在飞去重。
  const refresh = useCallback(() => run({ force: true }), [run])
  return { data, loading, error, refresh }
}
