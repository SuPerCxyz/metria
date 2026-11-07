import '../../test-support/setup-dom.js'

import assert from 'node:assert/strict'
import test from 'node:test'
import React from 'react'
import { render, waitFor } from '@testing-library/react'

import { AuthProvider, useAuth } from './useAuth.jsx'

function AuthProbe() {
  const { user, authed } = useAuth()
  return React.createElement(
    'span',
    { 'data-testid': 'auth' },
    `${authed}:${user?.username || ''}`,
  )
}

test('AuthProvider requests /auth/me once and shares the profile', async () => {
  const originalFetch = globalThis.fetch
  localStorage.setItem('metria-token', 'test-token')
  let calls = 0
  globalThis.fetch = async (url) => {
    calls += 1
    assert.ok(String(url).includes('/auth/me'))
    return {
      ok: true,
      status: 200,
      json: async () => ({ username: 'admin', display_name: 'Admin' }),
    }
  }
  try {
    const view = render(
      React.createElement(AuthProvider, null, React.createElement(AuthProbe)),
    )
    await waitFor(() =>
      assert.equal(view.getByTestId('auth').textContent, 'true:admin'),
    )
    assert.equal(calls, 1, '应用生命周期内 /auth/me 只应请求一次')
    view.unmount()
  } finally {
    globalThis.fetch = originalFetch
    localStorage.removeItem('metria-token')
  }
})
