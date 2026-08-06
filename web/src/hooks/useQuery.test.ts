import { beforeEach, describe, expect, it } from 'vitest'
import { defaultRange, quickRange, setRange, getRange, type Range } from './useQuery'

describe('range helpers', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('provides a default 7-day range', () => {
    const r = defaultRange()
    expect(r.from).toBeTruthy()
    expect(r.to).toBeTruthy()
    expect(r.timezone).toBeTruthy()
    expect(r.granularity).toBe('hour')
    const span = new Date(r.to).getTime() - new Date(r.from).getTime()
    expect(span).toBeCloseTo(7 * 24 * 3600 * 1000, -3)
  })

  it('quickRange 今天 starts at midnight', () => {
    const r = quickRange('今天')
    const d = new Date(r.from)
    expect(d.getHours()).toBe(0)
    expect(d.getMinutes()).toBe(0)
  })

  it('quickRange 最近 24 小时 spans one day', () => {
    const r = quickRange('最近 24 小时')
    const span = new Date(r.to).getTime() - new Date(r.from).getTime()
    expect(span).toBeCloseTo(24 * 3600 * 1000, -3)
  })

  it('setRange persists and getRange returns it', () => {
    const r: Range = {
      from: '2026-08-01T00:00:00Z',
      to: '2026-08-02T00:00:00Z',
      timezone: 'Asia/Shanghai',
      granularity: 'day',
    }
    setRange(r)
    const saved = localStorage.getItem('metria-range')
    expect(saved).toBeTruthy()
    const got = getRange()
    expect(got.from).toBe(r.from)
    expect(got.granularity).toBe('day')
  })

  it('quickRange default falls back to 7 days', () => {
    const r = quickRange('未知标签')
    const span = new Date(r.to).getTime() - new Date(r.from).getTime()
    expect(span).toBeCloseTo(7 * 24 * 3600 * 1000, -3)
  })
})
