// Metria API 对接层：封装 /api/v1 后端，统一鉴权、错误与查询参数。

const API = '/api/v1'

const storage = typeof localStorage !== 'undefined' ? localStorage : null
let token = storage ? storage.getItem('metria-token') : null
let activeSignal = null

/**
 * 在当前同步调用栈内绑定 AbortSignal；api() 会把 signal 透传给 fetch。
 * useQuery 的 fetcher 是同步调用 api 的，因此无需改写每个页面请求。
 */
export function withAbortSignal(signal, fn) {
  const previous = activeSignal
  activeSignal = signal || null
  try {
    return fn()
  } finally {
    activeSignal = previous
  }
}

export function getToken() {
  return token || (storage ? storage.getItem('metria-token') : null)
}

export function setToken(t) {
  token = t
  if (!storage) return
  if (t) storage.setItem('metria-token', t)
  else storage.removeItem('metria-token')
}

export class ApiError extends Error {
  constructor(status, message) {
    super(message)
    this.status = status
  }
}

export async function api(path, opts = {}) {
  const headers = { ...(opts.headers || {}) }
  if (token) headers['Authorization'] = `Bearer ${token}`
  if (opts.body && !headers['Content-Type']) headers['Content-Type'] = 'application/json'
  const res = await fetch(`${API}${path}`, {
    ...opts,
    headers,
    signal: opts.signal || activeSignal || undefined,
  })
  if (res.status === 401) {
    if (typeof window !== 'undefined') {
      window.dispatchEvent(new CustomEvent('metria:unauth'))
    }
    throw new ApiError(401, '未登录')
  }
  if (!res.ok) {
    let message = res.statusText
    try {
      const body = await res.json()
      message = body.message || body.error || message
    } catch {
      /* ignore */
    }
    throw new ApiError(res.status, message)
  }
  return res.json()
}

export const q = (params) => {
  const sp = new URLSearchParams()
  for (const [k, v] of Object.entries(params || {})) {
    if (v !== undefined && v !== null && v !== '') sp.set(k, String(v))
  }
  const s = sp.toString()
  return s ? `?${s}` : ''
}

// 公开端点（分享页）无需 token
export async function apiPublic(path) {
  const res = await fetch(`${API}${path}`)
  if (!res.ok) {
    const body = await res.json().catch(() => null)
    throw new ApiError(res.status, body?.message || body?.error || `HTTP ${res.status}`)
  }
  return res.json()
}

// 时间范围 → 查询参数
export function rangeParams(range) {
  return {
    from: range?.from || undefined,
    to: range?.to || undefined,
    timezone: range?.timezone || Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
  }
}

// 时间范围 + 全局用量筛选，所有聚合页面使用同一套参数。
export function usageRangeParams(range, filters = {}) {
  return {
    ...rangeParams(range),
    node_id: filters.nodeId || undefined,
    client_id: filters.clientId || undefined,
    model: filters.model || undefined,
    include_subagents: filters.includeSubagents ? true : undefined,
  }
}
