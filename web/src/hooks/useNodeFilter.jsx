// 全局节点过滤器：全部节点 / 单个节点。状态会话级跨页面保持，统计页面查询自动跟随。

import React, { createContext, useCallback, useContext, useMemo, useState } from 'react'

const NodeFilterContext = createContext(null)

export function NodeFilterProvider({ children }) {
  const [nodeId, setNodeId] = useState(null) // null = 全部节点
  const [clientId, setClientId] = useState(null)
  const [model, setModel] = useState(null)

  const setNode = useCallback((next) => setNodeId(next || null), [])
  const setClient = useCallback((next) => setClientId(next || null), [])
  const setModelValue = useCallback((next) => setModel(next || null), [])
  const clear = useCallback(() => {
    setNodeId(null)
    setClientId(null)
    setModel(null)
  }, [])

  const value = useMemo(() => ({
    nodeId,
    clientId,
    model,
    setNodeId: setNode,
    setClientId: setClient,
    setModel: setModelValue,
    clearFilters: clear,
  }), [nodeId, clientId, model, setNode, setClient, setModelValue, clear])
  return <NodeFilterContext.Provider value={value}>{children}</NodeFilterContext.Provider>
}

export function useNodeFilter() {
  const ctx = useContext(NodeFilterContext)
  // 未包裹 Provider 时退化为「全部节点」，保证组件可独立复用
  if (!ctx) return {
    nodeId: null,
    clientId: null,
    model: null,
    setNodeId: () => {},
    setClientId: () => {},
    setModel: () => {},
    clearFilters: () => {},
  }
  return ctx
}
