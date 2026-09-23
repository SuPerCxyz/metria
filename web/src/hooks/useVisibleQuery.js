// 区域可见后才发起的查询：容器首次进入视口激活；顶栏手动刷新可越过可见性门控。
// 激活后与普通 useQuery 行为一致（范围切换、刷新照常请求），激活是一次性的单向门。

import { useCallback, useEffect, useState } from 'react'
import { useQuery } from './useQuery'

/**
 * 用法：`const perf = useVisibleQuery(key, fetcher)`，再把 `perf.ref` 挂到
 * 所在区域的容器元素上；data / loading / error / refresh 与 useQuery 一致。
 * 未激活时 loading 恒为 true，避免把「还没请求」误报成「已加载的空数据」。
 */
export function useVisibleQuery(key, fetcher) {
  const [container, setContainer] = useState(null)
  const [activated, setActivated] = useState(false)

  // 稳定的回调 ref：仅在元素挂载/卸载时触发，容器条件渲染（如标签页切换）也能接上。
  const ref = useCallback((node) => { setContainer(node) }, [])

  // 首次进入视口即激活；环境不支持 IntersectionObserver 时退化为立即请求，宁可多发也不缺数据。
  useEffect(() => {
    if (activated || !container) return undefined
    if (typeof IntersectionObserver === 'undefined') {
      setActivated(true)
      return undefined
    }
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) setActivated(true)
    })
    observer.observe(container)
    return () => observer.disconnect()
  }, [container, activated])

  // 顶栏手动刷新越过可见性门控：先置为已激活，再由 useQuery 的 enabled 效果发起请求；
  // useQuery 自身的刷新监听在事件发生时看到 enabled 仍为 false 会跳过，避免双发。
  useEffect(() => {
    const onRefresh = () => setActivated(true)
    window.addEventListener('metria:refresh', onRefresh)
    return () => window.removeEventListener('metria:refresh', onRefresh)
  }, [])

  const query = useQuery(key, fetcher, { enabled: activated })
  return {
    ...query,
    ref,
    ready: activated,
    loading: activated ? query.loading : true,
  }
}
