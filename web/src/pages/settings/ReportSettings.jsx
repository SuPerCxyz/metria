// 设置页「用量报告」区块：全局时区、收件人、SMTP/Webhook 渠道、三独立调度、测试发送与发送历史。

import React, { useEffect, useState } from 'react'
import { api } from '../../services/api'
import { LoadingSkeleton, ErrorState } from '../../components/feedback/Feedback'

const COMMON_TZ = [
  'Asia/Shanghai',
  'Asia/Tokyo',
  'Asia/Singapore',
  'Europe/London',
  'Europe/Berlin',
  'America/New_York',
  'America/Los_Angeles',
  'UTC',
]
const WEEKDAYS = ['周一', '周二', '周三', '周四', '周五', '周六', '周日']
const TLS_OPTIONS = [
  { value: 'starttls', label: 'STARTTLS（587）' },
  { value: 'tls', label: 'SSL/TLS（465）' },
  { value: 'none', label: '无加密（明文）' },
]

function emptyDraft() {
  return {
    email_enabled: false,
    webhook_enabled: false,
    recipients: '',
    smtp: { host: '', port: 587, username: '', password: '', from: '', tls: 'starttls' },
    webhook: { url: '', headers: [], secret: '' },
    schedules: {
      daily: { enabled: false, time: '12:00' },
      weekly: { enabled: false, time: '12:00', weekday: 0 },
      monthly: { enabled: false, time: '12:00', day: 1 },
    },
    attachments_enabled: true,
  }
}

function mergeConfig(cfg) {
  const base = emptyDraft()
  if (!cfg) return base
  return {
    ...base,
    ...cfg,
    smtp: { ...base.smtp, ...(cfg.smtp || {}), password: '' },
    webhook: { ...base.webhook, ...(cfg.webhook || {}) },
    schedules: {
      daily: { ...base.schedules.daily, ...(cfg.schedules?.daily || {}) },
      weekly: { ...base.schedules.weekly, ...(cfg.schedules?.weekly || {}) },
      monthly: { ...base.schedules.monthly, ...(cfg.schedules?.monthly || {}) },
    },
  }
}

const inputCls =
  'w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700 focus:border-indigo-500 focus:ring-2 focus:ring-indigo-500/30 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-200'
const cardCls =
  'bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6'

function Label({ children }) {
  return <span className="mb-1 block text-xs font-medium text-gray-500 dark:text-gray-400">{children}</span>
}

function Toggle({ checked, onChange, label }) {
  return (
    <label className="inline-flex items-center gap-2 cursor-pointer select-none">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} className="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500" />
      <span className="text-sm text-gray-700 dark:text-gray-200">{label}</span>
    </label>
  )
}

export default function ReportSettings() {
  const [draft, setDraft] = useState(emptyDraft())
  const [timezone, setTimezone] = useState('')
  const [meta, setMeta] = useState({ smtp_password_set: false, resolved_recipients: [], timezone_default: '' })
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [saving, setSaving] = useState(false)
  const [testing, setTesting] = useState(false)
  const [notice, setNotice] = useState('')
  const [testResults, setTestResults] = useState(null)
  const [history, setHistory] = useState([])

  async function load() {
    setLoading(true)
    setError('')
    try {
      const [res, hist] = await Promise.all([
        api('/settings/report'),
        api('/settings/report/history').catch(() => ({ sends: [] })),
      ])
      setDraft(mergeConfig(res.config))
      setTimezone(res.timezone || res.timezone_default || '')
      setMeta({
        smtp_password_set: !!res.smtp_password_set,
        resolved_recipients: res.resolved_recipients || [],
        timezone_default: res.timezone_default || '',
      })
      setHistory(hist.sends || [])
    } catch (e) {
      setError(e.message || '加载失败')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    load()
  }, [])

  const set = (patch) => setDraft((d) => ({ ...d, ...patch }))
  const setSched = (key, patch) =>
    setDraft((d) => ({ ...d, schedules: { ...d.schedules, [key]: { ...d.schedules[key], ...patch } } }))

  async function save() {
    setSaving(true)
    setNotice('')
    setError('')
    try {
      const payload = {
        ...draft,
        timezone,
        smtp: { ...draft.smtp, password: draft.smtp.password ? draft.smtp.password : null },
      }
      await api('/settings/report', { method: 'PUT', body: JSON.stringify(payload) })
      setNotice('已保存')
      await load()
    } catch (e) {
      setError(e.message || '保存失败')
    } finally {
      setSaving(false)
    }
  }

  async function sendTest() {
    setTesting(true)
    setNotice('')
    setError('')
    setTestResults(null)
    try {
      const res = await api('/settings/report/test', { method: 'POST' })
      setTestResults(res.results || [])
      setNotice(res.ok ? '测试发送已完成' : '测试发送未成功，请查看结果')
      const hist = await api('/settings/report/history').catch(() => ({ sends: [] }))
      setHistory(hist.sends || [])
    } catch (e) {
      setError(e.message || '测试发送失败')
    } finally {
      setTesting(false)
    }
  }

  if (loading) return <LoadingSkeleton rows={5} />

  return (
    <div className="space-y-4">
      <div className={cardCls}>
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">用量报告</h2>
        <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
          按日/周/月把 Token 使用情况推送到邮箱或 Webhook。估算项在报告中统一标注「估算」，缺失口径不显示。
        </p>
        {error && <div className="mt-3 text-sm text-rose-600 dark:text-rose-400">{error}</div>}
        {notice && <div className="mt-3 text-sm text-emerald-600 dark:text-emerald-400">{notice}</div>}

        <div className="mt-5 grid grid-cols-1 gap-4 md:grid-cols-2">
          <label>
            <Label>全局时区（作用于展示与报告分桶）</Label>
            <input list="metria-tz" value={timezone} onChange={(e) => setTimezone(e.target.value)} placeholder="Asia/Shanghai" className={inputCls} />
            <datalist id="metria-tz">
              {COMMON_TZ.map((tz) => (
                <option key={tz} value={tz} />
              ))}
            </datalist>
            <span className="mt-1 block text-xs text-gray-400 dark:text-gray-500">环境默认：{meta.timezone_default || '—'}</span>
          </label>
          <label>
            <Label>收件人（逗号分隔，留空自动解析）</Label>
            <input value={draft.recipients} onChange={(e) => set('recipients', e.target.value)} placeholder="留空则使用 OIDC 邮箱" className={inputCls} />
            <span className="mt-1 block text-xs text-gray-400 dark:text-gray-500">
              自动解析：{meta.resolved_recipients.length ? meta.resolved_recipients.join(', ') : '未解析到邮箱'}
            </span>
          </label>
        </div>
      </div>

      <div className={cardCls}>
        <div className="flex items-center justify-between">
          <h3 className="text-base font-bold text-gray-800 dark:text-gray-100">邮件渠道（SMTP）</h3>
          <Toggle checked={draft.email_enabled} onChange={(v) => set('email_enabled', v)} label="启用" />
        </div>
        <div className="mt-4 grid grid-cols-1 gap-4 md:grid-cols-2">
          <label>
            <Label>SMTP 服务器</Label>
            <input value={draft.smtp.host} onChange={(e) => set({ smtp: { ...draft.smtp, host: e.target.value } })} placeholder="smtp.example.com" className={inputCls} />
          </label>
          <label>
            <Label>端口</Label>
            <input type="number" value={draft.smtp.port} onChange={(e) => set({ smtp: { ...draft.smtp, port: Number(e.target.value) } })} className={inputCls} />
          </label>
          <label>
            <Label>用户名</Label>
            <input value={draft.smtp.username} onChange={(e) => set({ smtp: { ...draft.smtp, username: e.target.value } })} className={inputCls} />
          </label>
          <label>
            <Label>密码{meta.smtp_password_set ? '（已设置，留空保持不变）' : ''}</Label>
            <input type="password" value={draft.smtp.password} onChange={(e) => set({ smtp: { ...draft.smtp, password: e.target.value } })} placeholder={meta.smtp_password_set ? '••••••••' : ''} className={inputCls} />
          </label>
          <label>
            <Label>发件人</Label>
            <input value={draft.smtp.from} onChange={(e) => set({ smtp: { ...draft.smtp, from: e.target.value } })} placeholder="Metria <metria@example.com>" className={inputCls} />
          </label>
          <label>
            <Label>加密方式</Label>
            <select value={draft.smtp.tls} onChange={(e) => set({ smtp: { ...draft.smtp, tls: e.target.value } })} className={inputCls}>
              {TLS_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
          </label>
        </div>
      </div>

      <div className={cardCls}>
        <div className="flex items-center justify-between">
          <h3 className="text-base font-bold text-gray-800 dark:text-gray-100">Webhook 渠道（通用 JSON）</h3>
          <Toggle checked={draft.webhook_enabled} onChange={(v) => set('webhook_enabled', v)} label="启用" />
        </div>
        <div className="mt-4 grid grid-cols-1 gap-4 md:grid-cols-2">
          <label>
            <Label>URL</Label>
            <input value={draft.webhook.url} onChange={(e) => set({ webhook: { ...draft.webhook, url: e.target.value } })} placeholder="https://example.com/hook" className={inputCls} />
          </label>
          <label>
            <Label>密钥（可选，作为 X-Metria-Secret）</Label>
            <input value={draft.webhook.secret} onChange={(e) => set({ webhook: { ...draft.webhook, secret: e.target.value } })} className={inputCls} />
          </label>
        </div>
      </div>

      <div className={cardCls}>
        <h3 className="text-base font-bold text-gray-800 dark:text-gray-100">周期调度</h3>
        <p className="mt-1 text-xs text-gray-400 dark:text-gray-500">三个调度相互独立，可同时启用；按全局时区汇总上一完整周期。</p>
        <div className="mt-4 space-y-4">
          <ScheduleRow label="每日" hint="汇总上一自然日" sched={draft.schedules.daily} onChange={(p) => setSched('daily', p)} />
          <ScheduleRow label="每周" hint="汇总上一自然周" sched={draft.schedules.weekly} onChange={(p) => setSched('weekly', p)} weekly />
          <ScheduleRow label="每月" hint="汇总上一自然月" sched={draft.schedules.monthly} onChange={(p) => setSched('monthly', p)} monthly />
        </div>
        <div className="mt-4 border-t border-gray-100 dark:border-gray-700/60 pt-4">
          <Toggle checked={draft.attachments_enabled} onChange={(v) => set('attachments_enabled', v)} label="邮件附带图表与 PDF" />
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-3">
        <button onClick={save} disabled={saving} className="rounded-lg bg-indigo-600 px-4 py-2 text-sm text-white hover:bg-indigo-700 disabled:opacity-50">
          {saving ? '保存中…' : '保存配置'}
        </button>
        <button onClick={sendTest} disabled={testing} className="rounded-lg border border-gray-300 dark:border-gray-600 px-4 py-2 text-sm text-gray-700 dark:text-gray-200 hover:bg-gray-50 dark:hover:bg-gray-700/40 disabled:opacity-50">
          {testing ? '发送中…' : '发送测试邮件/Webhook'}
        </button>
      </div>

      {testResults && (
        <div className={cardCls}>
          <h3 className="text-base font-bold text-gray-800 dark:text-gray-100">测试发送结果</h3>
          <ul className="mt-3 space-y-2 text-sm">
            {testResults.map((r, i) => (
              <li key={i} className={r.ok ? 'text-emerald-600 dark:text-emerald-400' : 'text-rose-600 dark:text-rose-400'}>
                {r.channel}：{r.ok ? '成功' : `失败（${r.detail || '未知错误'}）`}
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className={cardCls}>
        <div className="flex items-center justify-between">
          <h3 className="text-base font-bold text-gray-800 dark:text-gray-100">发送历史</h3>
          <button onClick={load} className="text-xs text-indigo-600 hover:underline dark:text-indigo-400">
            刷新
          </button>
        </div>
        {history.length === 0 ? (
          <p className="mt-3 text-sm text-gray-400 dark:text-gray-500">暂无发送记录。</p>
        ) : (
          <div className="mt-3 overflow-x-auto">
            <table className="min-w-full text-sm">
              <thead>
                <tr className="text-left text-xs text-gray-400 dark:text-gray-500">
                  <th className="py-1 pr-4">时间</th>
                  <th className="py-1 pr-4">周期</th>
                  <th className="py-1 pr-4">渠道</th>
                  <th className="py-1 pr-4">结果</th>
                  <th className="py-1">详情</th>
                </tr>
              </thead>
              <tbody>
                {history.map((h, i) => (
                  <tr key={i} className="border-t border-gray-100 dark:border-gray-700/60">
                    <td className="py-1.5 pr-4 whitespace-nowrap">{h.created_at?.replace('T', ' ').slice(0, 19)}</td>
                    <td className="py-1.5 pr-4">{h.kind} / {h.period}</td>
                    <td className="py-1.5 pr-4">{h.channel}</td>
                    <td className={`py-1.5 pr-4 ${h.status === 'success' ? 'text-emerald-600 dark:text-emerald-400' : 'text-rose-600 dark:text-rose-400'}`}>{h.status}</td>
                    <td className="py-1.5 text-gray-500 dark:text-gray-400">{h.detail || '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  )
}

function ScheduleRow({ label, hint, sched, onChange, weekly = false, monthly = false }) {
  return (
    <div className="flex flex-wrap items-end gap-4">
      <div className="min-w-[120px]">
        <div className="text-sm font-medium text-gray-700 dark:text-gray-200">{label}</div>
        <div className="text-xs text-gray-400 dark:text-gray-500">{hint}</div>
      </div>
      <Toggle checked={sched.enabled} onChange={(v) => onChange({ enabled: v })} label="启用" />
      <label>
        <Label>发送时间</Label>
        <input type="time" value={sched.time} onChange={(e) => onChange({ time: e.target.value })} className={inputCls} />
      </label>
      {weekly && (
        <label className="block w-28 shrink-0">
          <Label>周几</Label>
          <select value={sched.weekday ?? 0} onChange={(e) => onChange({ weekday: Number(e.target.value) })} className={inputCls}>
            {WEEKDAYS.map((w, i) => (
              <option key={w} value={i}>
                {w}
              </option>
            ))}
          </select>
        </label>
      )}
      {monthly && (
        <label className="block w-24 shrink-0">
          <Label>几号（1–28）</Label>
          <input type="number" min={1} max={28} value={sched.day ?? 1} onChange={(e) => onChange({ day: Number(e.target.value) })} className={inputCls} />
        </label>
      )}
    </div>
  )
}
