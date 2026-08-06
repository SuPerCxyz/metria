import { useState } from 'preact/hooks'
import { api } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { useQuery } from '../hooks/useQuery'
import { t } from '../lib/i18n'

/** 分享：创建公开只读链接并管理。 */
export function Shares() {
  const shares = useQuery<any>('/shares', () => api('/shares'))
  const [kind, setKind] = useState('session')
  const [targetId, setTargetId] = useState('')
  const [msg, setMsg] = useState('')

  const create = async () => {
    if (!targetId) {
      setMsg(t('shares.hint'))
      return
    }
    try {
      const r = await api<any>('/shares', {
        method: 'POST',
        body: JSON.stringify({ kind, target_id: targetId }),
      })
      setMsg(`分享链接：${window.location.origin}${r.url}`)
      shares.refresh()
    } catch (e) {
      setMsg(`创建失败：${(e as Error).message}`)
    }
  }

  if (shares.error) return <ErrorBox error={shares.error} onRetry={shares.refresh} />

  return (
    <div class="page">
      <h2>{t('nav.shares')}</h2>
      <p class="page-note">分享链接为公开只读视图，返回脱敏 DTO（不含正文与敏感信息）。</p>
      {msg && <div class="state-box">{msg}</div>}

      <Card title={t('shares.create')}>
        <div class="form-grid">
          <label>
            类型
            <select value={kind} onChange={(e) => setKind((e.target as HTMLSelectElement).value)}>
              <option value="session">Session</option>
              <option value="node">Node</option>
            </select>
          </label>
          <label>
            目标 ID
            <input value={targetId} onInput={(e) => setTargetId((e.target as HTMLInputElement).value)} placeholder={t('shares.hint')} />
          </label>
        </div>
        <button type="button" class="btn primary" onClick={create}>
          创建
        </button>
      </Card>

      <Card title={t('shares.list')}>
        {(shares.data?.shares || []).length === 0 && <Empty text={t('shares.empty')} />}
        <table class="table">
          <thead>
            <tr>
              <th>Slug</th>
              <th>类型</th>
              <th>目标</th>
              <th>创建时间</th>
              <th>链接</th>
            </tr>
          </thead>
          <tbody>
            {(shares.data?.shares || []).map((s: any) => (
              <tr key={s.slug}>
                <td class="mono">{s.slug}</td>
                <td>{s.kind}</td>
                <td class="mono">{s.target_id}</td>
                <td>{s.created_at}</td>
                <td>
                  <a href={`/s/${s.slug}`} target="_blank" rel="noreferrer">
                    /s/{s.slug}
                  </a>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  )
}
