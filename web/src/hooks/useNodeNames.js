// 节点 ID → 显示名映射：从 /nodes 拉取一次并缓存，供列表/详情展示改名后的节点名。

import { useEffect, useState } from 'react'
import { api } from '../services/api'

let cached = null
let inflight = null

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
