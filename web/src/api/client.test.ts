import { describe, expect, it } from 'vitest'
import { q } from './client'

describe('api query serialization', () => {
  it('serializes params to query string', () => {
    expect(q({ a: '1', b: 'x' })).toBe('?a=1&b=x')
  })

  it('omits undefined and empty values', () => {
    expect(q({ a: '1', b: undefined, c: '' })).toBe('?a=1')
  })

  it('encodes special chars', () => {
    expect(q({ from: '2026-08-01T00:00:00Z' })).toBe('?from=2026-08-01T00%3A00%3A00Z')
  })

  it('returns empty string for no params', () => {
    expect(q({})).toBe('')
    expect(q({ a: undefined })).toBe('')
  })

  it('supports numbers', () => {
    expect(q({ limit: 8 })).toBe('?limit=8')
  })
})
