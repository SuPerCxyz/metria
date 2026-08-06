// 错误边界：捕获渲染错误，避免整树崩溃。

import React from 'react'

export default class ErrorBoundary extends React.Component {
  constructor(props) {
    super(props)
    this.state = { error: null }
  }

  static getDerivedStateFromError(error) {
    return { error }
  }

  componentDidCatch(error, info) {
    console.error('Metria render error:', error, info)
  }

  render() {
    if (this.state.error) {
      return (
        <div className="min-h-screen flex items-center justify-center bg-gray-100 dark:bg-gray-900 px-4">
          <div className="max-w-md w-full bg-white dark:bg-gray-800 rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
            <h2 className="text-lg font-bold text-red-600 dark:text-red-400 mb-2">页面加载出错</h2>
            <p className="text-sm text-gray-600 dark:text-gray-300 break-all">{String(this.state.error?.message || this.state.error)}</p>
            <button type="button" onClick={() => window.location.reload()} className="mt-4 px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-700 text-white text-sm">
              刷新
            </button>
          </div>
        </div>
      )
    }
    return this.props.children
  }
}
