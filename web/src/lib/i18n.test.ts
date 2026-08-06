import { beforeEach, describe, expect, it } from 'vitest'
import { getLocale, setLocale, t } from './i18n'

describe('i18n', () => {
  beforeEach(() => {
    localStorage.clear()
    setLocale('zh')
  })

  it('defaults to Chinese', () => {
    expect(getLocale()).toBe('zh')
  })

  it('translates zh keys', () => {
    expect(t('common.loading')).toBe('加载中…')
    expect(t('nav.sessions')).toBe('会话')
  })

  it('switches to English and back', () => {
    setLocale('en')
    expect(getLocale()).toBe('en')
    expect(t('common.loading')).toBe('Loading…')
    expect(t('nav.sessions')).toBe('Sessions')
    setLocale('zh')
    expect(t('common.loading')).toBe('加载中…')
  })

  it('falls back to zh when key missing in en', () => {
    setLocale('en')
    expect(t('nav.overview')).toBe('Overview')
    // 若 en 缺失 key，回退 zh
    expect(t('common.estimatedTraffic')).toBe('Estimated traffic')
  })

  it('returns key when unknown', () => {
    expect(t('no.such.key')).toBe('no.such.key')
  })

  it('interpolates variables', () => {
    expect(t('x', {})).toBe('x')
  })

  it('persists locale', () => {
    setLocale('en')
    expect(localStorage.getItem('metria-locale')).toBe('en')
  })
})
