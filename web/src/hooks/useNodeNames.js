// 节点 ID → 显示名映射：从 /nodes 拉取一次并缓存，供列表/详情展示改名后的节点名。

import { useEffect, useState } from 'react'
import { api } from '../services/api'

let cached = null
let inflight = null

/**
 * 用 filter-options 返回的节点列表预热缓存，避免同一页面既请求 /nodes
 * 又依赖 filter-options 时出现两份不一致的节点数据。
 */
export function primeNodeNames(nodes) {
  if (!Array.isArray(nodes) || nodes.length === 0) return
  const map = {}
  for (const node of nodes) {
    if (node && node.id) map[node.id] = node.label || node.name || node.id
  }
  if (Object.keys(map).length === 0) return
  cached = cached ? { ...cached, ...map } : map
}

export function useNodeNames() {
  const [names, setNames] = useState(cached || {})

  useEffect(() => {
    if (cached) {
      setNames(cached)
      return
    }
    if (!inflight) {
      inflight = api('/nodes')
        .then((d) => {
          const map = {}
          for (const n of d.nodes || []) map[n.id] = n.name || n.id
          cached = map
          return map
        })
        .catch((e) => {
          inflight = null
          throw e
        })
    }
    inflight.then((map) => setNames(map)).catch(() => {})
  }, [])

  return names
}
