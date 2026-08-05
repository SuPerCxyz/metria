export const API = '/api/v1'

let token: string | null = localStorage.getItem('metria-token')

export function getToken(): string | null {
  return token
}

export function setToken(t: string | null) {
  token = t
  if (t) localStorage.setItem('metria-token', t)
  else localStorage.removeItem('metria-token')
}

export class ApiError extends Error {
  status: number
  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

export async function api<T = unknown>(path: string, opts: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = { ...(opts.headers as Record<string, string> | undefined) }
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
  return res.json() as Promise<T>
}

export const q = (params: Record<string, string | number | undefined>) => {
  const sp = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== '') sp.set(k, String(v))
  }
  const s = sp.toString()
  return s ? `?${s}` : ''
}
