// Metria API 对接层：封装 /api/v1 后端，统一鉴权、错误与查询参数。

const API = '/api/v1'

let token = localStorage.getItem('metria-token')

export function getToken() {
  return token || localStorage.getItem('metria-token')
}

export function setToken(t) {
  token = t
  if (t) localStorage.setItem('metria-token', t)
  else localStorage.removeItem('metria-token')
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
  const res = await fetch(`${API}${path}`, { ...opts, headers })
  if (res.status === 401) {
    window.dispatchEvent(new CustomEvent('metria:unauth'))
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
    timezone: range?.timezone || undefined,
  }
}
