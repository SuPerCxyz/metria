// 页面标题元信息：各页面通过 PageHeader 设置，Header 顶栏读取展示。

import { createContext, useContext, useState, useCallback } from 'react'

const PageMetaContext = createContext({
  title: '',
  subtitle: null,
  setMeta: () => {},
})

export function PageMetaProvider({ children }) {
  const [meta, setMetaState] = useState({ title: '', subtitle: null })
  const setMeta = useCallback((m) => setMetaState(m), [])
  return (
    <PageMetaContext.Provider value={{ ...meta, setMeta }}>
      {children}
    </PageMetaContext.Provider>
  )
}

export function usePageMeta() {
  return useContext(PageMetaContext)
}
