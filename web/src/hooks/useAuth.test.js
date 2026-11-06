import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (relative) => readFileSync(new URL(relative, import.meta.url), 'utf8')

test('/auth/me is owned by AuthProvider, not Header/Settings', () => {
  const provider = read('./useAuth.jsx')
  assert.equal((provider.match(/api\('\/auth\/me'\)/g) || []).length, 1)
  assert.equal(read('../partials/Header.jsx').includes("api('/auth/me')"), false)
  assert.equal(read('../pages/settings/Settings.jsx').includes("api('/auth/me')"), false)
  assert.ok(read('../App.jsx').includes('AuthProvider'))
})
