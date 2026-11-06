// 局部错误边界：捕获子树渲染/懒加载错误，不影响页面其余部分。

import React from 'react'

export default class InlineErrorBoundary extends React.Component {
  constructor(props) {
    super(props)
    this.state = { error: null }
    this.reset = this.reset.bind(this)
  }

  static getDerivedStateFromError(error) {
    return { error }
  }

  componentDidCatch(error, info) {
    console.error('Metria inline render error:', error, info)
  }

  reset() {
    this.setState({ error: null })
  }

  render() {
    if (this.state.error) {
      const { fallback } = this.props
      if (typeof fallback === 'function') {
        return fallback({ error: this.state.error, reset: this.reset })
      }
      return fallback || null
    }
    return this.props.children
  }
}
