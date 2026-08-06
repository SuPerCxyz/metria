import { describe, expect, it } from 'vitest'
import { fmtBytes, fmtTokens, fmtUsd, fmtDuration, pct } from './format'

describe('fmtBytes', () => {
  it('handles null/undefined as 0 B', () => {
    expect(fmtBytes(null)).toBe('0 B')
    expect(fmtBytes(undefined)).toBe('0 B')
  })
  it('formats B/KiB/MiB/GiB', () => {
    expect(fmtBytes(0)).toBe('0 B')
    expect(fmtBytes(512)).toBe('512 B')
    expect(fmtBytes(2048)).toBe('2.0 KiB')
    expect(fmtBytes(5 * 1024 * 1024)).toBe('5.0 MiB')
    expect(fmtBytes(3 * 1024 * 1024 * 1024)).toContain('GiB')
  })
})

describe('fmtTokens', () => {
  it('handles null as dash', () => {
    expect(fmtTokens(null)).toBe('—')
    expect(fmtTokens(undefined)).toBe('—')
  })
  it('formats k/M', () => {
    expect(fmtTokens(0)).toBe('0')
    expect(fmtTokens(999)).toBe('999')
    expect(fmtTokens(1500)).toBe('1.5k')
    expect(fmtTokens(2_500_000)).toBe('2.50M')
  })
})

describe('fmtUsd', () => {
  it('formats micro usd to dollars', () => {
    expect(fmtUsd(null)).toBe('$0.00')
    expect(fmtUsd(0)).toBe('$0.00')
    expect(fmtUsd(1_000_000)).toBe('$1.0000')
    expect(fmtUsd(123_456)).toBe('$0.1235')
  })
})

describe('fmtDuration', () => {
  it('returns dash without start', () => {
    expect(fmtDuration(null, null)).toBe('—')
  })
  it('computes seconds/minutes', () => {
    expect(fmtDuration('2026-08-06T00:00:00Z', '2026-08-06T00:00:02Z')).toBe('2.0s')
    expect(fmtDuration('2026-08-06T00:00:00Z', '2026-08-06T00:01:30Z')).toBe('1m30s')
  })
})

describe('pct', () => {
  it('guards zero total', () => {
    expect(pct(5, 0)).toBe(0)
  })
  it('computes rounded percent', () => {
    expect(pct(25, 100)).toBe(25)
    expect(pct(1, 3)).toBe(33)
  })
})
