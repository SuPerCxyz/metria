import { useEffect, useState } from 'preact/hooks'

export interface QueryState<T> {
  data?: T
  loading: boolean
  error?: string
  refresh: () => void
}

/** 轻量数据获取 hook：key 变化时自动重新请求。 */
export function useQuery<T>(key: string, fn: () => Promise<T>): QueryState<T> {
  const [state, setState] = useState<{ data?: T; loading: boolean; error?: string }>({
    loading: true,
  })
  const [tick, setTick] = useState(0)

  useEffect(() => {
    let alive = true
    setState((s) => ({ ...s, loading: true }))
    fn()
      .then((data) => alive && setState({ data, loading: false }))
      .catch((e) => alive && setState({ error: String(e?.message || e), loading: false }))
    return () => {
      alive = false
    }
  }, [key, tick])

  return { ...state, refresh: () => setTick((t) => t + 1) }
}

/** 全局时间范围。 */
export interface Range {
  from: string
  to: string
  timezone: string
  granularity: 'hour' | 'day'
}

export function defaultRange(): Range {
  const to = new Date()
  const from = new Date(to.getTime() - 7 * 24 * 3600 * 1000)
  return {
    from: from.toISOString(),
    to: to.toISOString(),
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'Asia/Shanghai',
    granularity: 'hour',
  }
}

/** 快捷范围。 */
export function quickRange(label: string): Range {
  const to = new Date()
  const r = defaultRange()
  r.to = to.toISOString()
  const oneDay = 24 * 3600 * 1000
  const now = to.getTime()
  switch (label) {
    case '今天':
      const today = new Date(now)
      today.setHours(0, 0, 0, 0)
      r.from = today.toISOString()
      break
    case '昨天':
      const y = new Date(now - oneDay)
      y.setHours(0, 0, 0, 0)
      const yend = new Date(now)
      yend.setHours(0, 0, 0, 0)
      r.from = y.toISOString()
      r.to = yend.toISOString()
      break
    case '最近 24 小时':
      r.from = new Date(now - oneDay).toISOString()
      break
    case '最近 7 天':
      r.from = new Date(now - 7 * oneDay).toISOString()
      break
    case '最近 30 天':
      r.from = new Date(now - 30 * oneDay).toISOString()
      break
    default:
      r.from = new Date(now - 7 * oneDay).toISOString()
  }
  return r
}

// ---- 全局时间范围 store ----
let range: Range = load()
const listeners = new Set<() => void>()

function load(): Range {
  try {
    const s = localStorage.getItem('metria-range')
    if (s) return { ...defaultRange(), ...JSON.parse(s) }
  } catch {
    /* ignore */
  }
  return defaultRange()
}

function emit() {
  listeners.forEach((l) => l())
}

export function getRange(): Range {
  return range
}

export function setRange(r: Range) {
  range = r
  localStorage.setItem('metria-range', JSON.stringify(r))
  emit()
}

export function useRangeStore(): Range {
  const [r, setR] = useState<Range>(range)
  useEffect(() => {
    const l = () => setR(range)
    listeners.add(l)
    return () => {
      listeners.delete(l)
    }
  }, [])
  return r
}
