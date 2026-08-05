import { useSyncExternalStore } from 'preact/compat'

/** 极简 hash 路由。 */
function subscribe(cb: () => void) {
  window.addEventListener('hashchange', cb)
  return () => window.removeEventListener('hashchange', cb)
}

function getHash(): string {
  return window.location.hash || '#/overview'
}

export interface Route {
  /** 一级路径，如 overview / nodes */
  path: string
  /** 路径段，如 ['nodes', 'node-01'] */
  parts: string[]
  id?: string
}

export function useRoute(): Route {
  const hash = useSyncExternalStore(subscribe, getHash)
  const parts = hash.replace(/^#\//, '').split('/').filter(Boolean)
  return {
    path: '/' + (parts[0] || 'overview'),
    parts,
    id: parts[1],
  }
}

export function nav(path: string) {
  window.location.hash = '/' + path
}

export function parseUrlParams(): Record<string, string> {
  const sp = new URLSearchParams(window.location.search)
  const out: Record<string, string> = {}
  sp.forEach((v, k) => {
    out[k] = v
  })
  return out
}
