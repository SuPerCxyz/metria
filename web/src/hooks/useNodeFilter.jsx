// 全局节点过滤器：全部节点 / 单个节点。状态会话级跨页面保持，统计页面查询自动跟随。

import React, { createContext, useCallback, useContext, useMemo, useState } from 'react'

const NodeFilterContext = createContext(null)

export function NodeFilterProvider({ children }) {
  const [nodeId, setNodeId] = useState(null) // null = 全部节点
  const [clientId, setClientId] = useState(null)
  const [model, setModel] = useState(null)
  const [projectId, setProjectId] = useState(null)

  const setNode = useCallback((next) => setNodeId(next || null), [])
  const setClient = useCallback((next) => setClientId(next || null), [])
  const setModelValue = useCallback((next) => setModel(next || null), [])
  const setProject = useCallback((next) => setProjectId(next || null), [])
  const clear = useCallback(() => {
    setNodeId(null)
    setClientId(null)
    setModel(null)
    setProjectId(null)
  }, [])

  const value = useMemo(() => ({
    nodeId,
    clientId,
    model,
    projectId,
    setNodeId: setNode,
    setClientId: setClient,
    setModel: setModelValue,
    setProjectId: setProject,
    clearFilters: clear,
  }), [nodeId, clientId, model, projectId, setNode, setClient, setModelValue, setProject, clear])
  return <NodeFilterContext.Provider value={value}>{children}</NodeFilterContext.Provider>
}

export function useNodeFilter() {
  const ctx = useContext(NodeFilterContext)
  // 未包裹 Provider 时退化为「全部节点」，保证组件可独立复用
  if (!ctx) return {
    nodeId: null,
    clientId: null,
    model: null,
    projectId: null,
    setNodeId: () => {},
    setClientId: () => {},
    setModel: () => {},
    setProjectId: () => {},
    clearFilters: () => {},
  }
  return ctx
}
