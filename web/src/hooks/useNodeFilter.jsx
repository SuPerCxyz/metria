// 全局节点过滤器：全部节点 / 单个节点。状态会话级跨页面保持，统计页面查询自动跟随。

import React, { createContext, useCallback, useContext, useMemo, useState } from 'react'

const NodeFilterContext = createContext(null)

export function NodeFilterProvider({ children }) {
  const [nodeId, setNodeId] = useState(null) // null = 全部节点

  const set = useCallback((next) => setNodeId(next || null), [])

  const value = useMemo(() => ({ nodeId, setNodeId: set }), [nodeId, set])
  return <NodeFilterContext.Provider value={value}>{children}</NodeFilterContext.Provider>
}

export function useNodeFilter() {
  const ctx = useContext(NodeFilterContext)
  // 未包裹 Provider 时退化为「全部节点」，保证组件可独立复用
  if (!ctx) return { nodeId: null, setNodeId: () => {} }
  return ctx
}
