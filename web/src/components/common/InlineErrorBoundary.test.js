import '../../../test-support/setup-dom.js'

import assert from 'node:assert/strict'
import test from 'node:test'
import React from 'react'
import { fireEvent, render, waitFor } from '@testing-library/react'

import InlineErrorBoundary from './InlineErrorBoundary.jsx'

test('renders the fallback and recovers after reset', async () => {
  const originalError = console.error
  console.error = () => {}
  try {
    let shouldThrow = true
    function Child() {
      if (shouldThrow) throw new Error('boom')
      return React.createElement('div', null, 'recovered')
    }
    const fallback = ({ reset }) => React.createElement(
      'button',
      {
        type: 'button',
        onClick: () => {
          shouldThrow = false
          reset()
        },
      },
      '重试',
    )

    const view = render(
      React.createElement(
        InlineErrorBoundary,
        { fallback },
        React.createElement(Child),
      ),
    )
    assert.ok(view.getByText('重试'), '子组件抛错时应渲染 fallback')
    fireEvent.click(view.getByText('重试'))
    await waitFor(() => assert.ok(view.getByText('recovered')))
  } finally {
    console.error = originalError
  }
})
