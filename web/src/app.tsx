import { useCallback, useState } from 'preact/hooks'

export type Theme = 'light' | 'dark'

function readTheme(): Theme {
  const saved = localStorage.getItem('metria-theme')
  if (saved === 'light' || saved === 'dark') return saved
  return window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

export function App() {
  const [theme, setTheme] = useState<Theme>(readTheme)
  const toggleTheme = useCallback(() => {
    setTheme((t) => {
      const next: Theme = t === 'light' ? 'dark' : 'light'
      localStorage.setItem('metria-theme', next)
      return next
    })
  }, [])

  return (
    <div data-theme={theme} class="app-shell">
      <header class="app-header">
        <span class="app-title">Metria</span>
        <span class="app-subtitle">AI 编程 Agent 用量监控 · 费用分析 · 流量估算</span>
        <button type="button" class="theme-toggle" onClick={toggleTheme}>
          {theme === 'light' ? '暗色' : '亮色'}
        </button>
      </header>
      <main class="app-main">
        <section class="placeholder-card">
          <h2>S0 工程骨架</h2>
          <p>Web 工作区已就绪，页面将在 S3 阶段实现。</p>
        </section>
      </main>
    </div>
  )
}
