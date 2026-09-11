// 设置页「用量报告」区块：全局时区、收件人、SMTP/Webhook 渠道、三独立调度、测试发送与发送历史。

import React, { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { api } from '../../services/api'
import { LoadingSkeleton, ErrorState } from '../../components/feedback/Feedback'
import { useToast } from '../../components/feedback/Toast'

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
  { value: 'starttls', label: 'STARTTLS' },
  { value: 'tls', label: 'SSL/TLS' },
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
const compactInputCls = inputCls.replace('w-full ', '')
const cardCls =
  'bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6'

function Label({ children }) {
  return <span className="mb-1 block text-xs font-medium text-gray-500 dark:text-gray-400">{children}</span>
}

function clampReportDay(value) {
  const day = Number(value)
  return Number.isFinite(day) ? Math.min(28, Math.max(1, Math.trunc(day))) : 1
}

function Toggle({ checked, onChange, label, disabled = false }) {
  return (
    <label className={`inline-flex items-center gap-2 select-none ${disabled ? 'cursor-not-allowed opacity-60' : 'cursor-pointer'}`}>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} className="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500" />
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
  const [testResults, setTestResults] = useState(null)
  const [history, setHistory] = useState([])
  const { notify } = useToast()
  const scheduleFieldRefs = useRef({})
  const [scheduleFieldWidth, setScheduleFieldWidth] = useState(null)

  useLayoutEffect(() => {
    const measure = () => {
      const widths = Object.values(scheduleFieldRefs.current)
        .filter(Boolean)
        .map(({ header, control }) => Math.ceil(Math.max(
          header?.getBoundingClientRect().width || 0,
          control?.getBoundingClientRect().width || 0,
        )))
        .filter(Boolean)
      if (widths.length !== 2) return
      const nextWidth = Math.max(...widths)
      setScheduleFieldWidth((current) => (current === nextWidth ? current : nextWidth))
    }

    measure()
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure)
    Object.values(scheduleFieldRefs.current).forEach(({ header, control }) => {
      if (header) observer?.observe(header)
      if (control) observer?.observe(control)
    })
    return () => observer?.disconnect()
  }, [loading])

  async function load(announce = false) {
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
      if (announce) notify('发送历史已刷新')
    } catch (e) {
      setError(e.message || '加载失败')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    load()
  }, [])

  const set = (keyOrPatch, value) => {
    const patch = typeof keyOrPatch === 'string' ? { [keyOrPatch]: value } : keyOrPatch
    setDraft((d) => ({ ...d, ...patch }))
  }
  const setSched = (key, patch) =>
    setDraft((d) => ({ ...d, schedules: { ...d.schedules, [key]: { ...d.schedules[key], ...patch } } }))
  const registerScheduleField = (name, part) => (element) => {
    scheduleFieldRefs.current[name] = { ...scheduleFieldRefs.current[name], [part]: element }
  }

  async function save() {
    setSaving(true)
    try {
      const payload = {
        ...draft,
        timezone,
        smtp: { ...draft.smtp, password: draft.smtp.password ? draft.smtp.password : null },
      }
      await api('/settings/report', { method: 'PUT', body: JSON.stringify(payload) })
      notify('已保存')
      await load()
    } catch (e) {
      notify(e.message || '保存失败', 'error')
    } finally {
      setSaving(false)
    }
  }

  async function toggleChannel(key, enabled) {
    const previous = draft
    const next = { ...draft, [key]: enabled }
    setDraft(next)
    setSaving(true)
    try {
      await api('/settings/report', {
        method: 'PUT',
        body: JSON.stringify({
          ...next,
          timezone,
          smtp: { ...next.smtp, password: next.smtp.password ? next.smtp.password : null },
        }),
      })
      notify(enabled ? '已启用' : '已停用')
    } catch (e) {
      setDraft(previous)
      notify(`启用状态保存失败：${e.message}`, 'error')
    } finally {
      setSaving(false)
    }
  }

  async function sendTest() {
    setTesting(true)
    setTestResults(null)
    try {
      const res = await api('/settings/report/test', { method: 'POST' })
      const results = res.results || []
      setTestResults(results)
      if (results.length > 0 && results.every((result) => result.ok)) notify('测试发送已完成')
      else notify('测试发送未成功，请查看结果', 'error')
      const hist = await api('/settings/report/history').catch(() => ({ sends: [] }))
      setHistory(hist.sends || [])
    } catch (e) {
      setTestResults([{ channel: 'system', ok: false, detail: e.message || '测试发送请求失败' }])
      notify(e.message || '测试发送请求失败', 'error')
    } finally {
      setTesting(false)
    }
  }

  if (loading) return <LoadingSkeleton rows={5} />
  if (error) return <ErrorState error={error} onRetry={() => load()} />
  const testFailed = !!testResults && (testResults.length === 0 || testResults.some((result) => !result.ok))

  return (
    <div className="space-y-4">
      <div className={cardCls}>
        <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">用量报告</h2>
        <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
          按日/周/月把 Token 使用情况推送到邮箱或 Webhook。估算项在报告中统一标注「估算」，缺失口径不显示。
        </p>

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
          <Toggle checked={draft.email_enabled} onChange={(v) => toggleChannel('email_enabled', v)} label="启用" disabled={saving} />
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
            <Label>用户名（SMTP 登录账号）</Label>
            <input value={draft.smtp.username} onChange={(e) => set({ smtp: { ...draft.smtp, username: e.target.value } })} placeholder="例如 metria@soocoo.xyz" className={inputCls} />
            <span className="mt-1 block text-xs text-gray-400 dark:text-gray-500">用于 SMTP 认证，通常填写完整邮箱地址。</span>
          </label>
          <label>
            <Label>密码{meta.smtp_password_set ? '（已设置，留空保持不变）' : ''}</Label>
            <input type="password" value={draft.smtp.password} onChange={(e) => set({ smtp: { ...draft.smtp, password: e.target.value } })} placeholder={meta.smtp_password_set ? '••••••••' : ''} className={inputCls} />
          </label>
          <label>
            <Label>发件人（邮件显示地址）</Label>
            <input value={draft.smtp.from} onChange={(e) => set({ smtp: { ...draft.smtp, from: e.target.value } })} placeholder="Metria <metria@example.com>" className={inputCls} />
            <span className="mt-1 block text-xs text-gray-400 dark:text-gray-500">收件人看到的地址，需获得 SMTP 账号授权。</span>
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
          <Toggle checked={draft.webhook_enabled} onChange={(v) => toggleChannel('webhook_enabled', v)} label="启用" disabled={saving} />
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
          <ScheduleRow
            label="每周"
            hint="汇总上一自然周"
            sched={draft.schedules.weekly}
            onChange={(p) => setSched('weekly', p)}
            fieldHeaderRef={registerScheduleField('weekday', 'header')}
            fieldControlRef={registerScheduleField('weekday', 'control')}
            fieldWidth={scheduleFieldWidth}
            weekly
          />
          <ScheduleRow
            label="每月"
            hint="汇总上一自然月"
            sched={draft.schedules.monthly}
            onChange={(p) => setSched('monthly', p)}
            fieldHeaderRef={registerScheduleField('month', 'header')}
            fieldControlRef={registerScheduleField('month', 'control')}
            fieldWidth={scheduleFieldWidth}
            monthly
          />
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
        <div className={`${cardCls} ${testFailed ? 'border-rose-300 dark:border-rose-700/70' : 'border-emerald-300 dark:border-emerald-700/70'}`}>
          <div className="flex items-center justify-between gap-3">
            <h3 className="text-base font-bold text-gray-800 dark:text-gray-100">测试发送结果</h3>
            <span className={`text-sm font-medium ${testFailed ? 'text-rose-600 dark:text-rose-400' : 'text-emerald-600 dark:text-emerald-400'}`}>
              {testFailed ? '发送未成功' : '发送成功'}
            </span>
          </div>
          {testResults.length === 0 ? (
            <p className="mt-3 text-sm text-rose-600 dark:text-rose-400">没有启用的发送渠道，未执行发送。</p>
          ) : (
            <ul className="mt-3 space-y-2 text-sm">
              {testResults.map((result, i) => (
                <li key={i} className={`rounded-lg border px-3 py-2 ${result.ok ? 'border-emerald-200 text-emerald-700 dark:border-emerald-800/70 dark:text-emerald-300' : 'border-rose-200 text-rose-700 dark:border-rose-800/70 dark:text-rose-300'}`}>
                  <div className="flex items-center justify-between gap-3">
                    <span className="font-medium">{result.channel}</span>
                    <span>{result.ok ? '成功' : '失败'}</span>
                  </div>
                  {!result.ok && <p className="mt-1 break-words text-xs">{result.detail || '未返回失败原因'}</p>}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      <div className={cardCls}>
        <div className="flex items-center justify-between">
          <h3 className="text-base font-bold text-gray-800 dark:text-gray-100">发送历史</h3>
          <button onClick={() => load(true)} className="text-xs text-indigo-600 hover:underline dark:text-indigo-400">
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

function ScheduleRow({ label, hint, sched, onChange, fieldHeaderRef, fieldControlRef, fieldWidth, weekly = false, monthly = false }) {
  const fieldStyle = fieldWidth ? { width: `${fieldWidth}px` } : { width: 'max-content' }
  const controlStyle = {
    width: fieldWidth ? '100%' : weekly ? 'calc(2em + 3.5rem)' : 'calc(2ch + 3.5rem)',
  }
  return (
    <div
      className="grid grid-cols-1 gap-x-4 gap-y-1 md:grid-rows-2 md:items-center md:grid-cols-[minmax(120px,max-content)_max-content_max-content_var(--schedule-field-width)]"
      style={{ '--schedule-field-width': fieldWidth ? `${fieldWidth}px` : 'max-content' }}
    >
      <div className="min-w-[120px] md:col-start-1 md:row-start-1">
        <div className="text-sm font-medium text-gray-700 dark:text-gray-200">{label}</div>
      </div>
      <div className="hidden md:block md:col-start-2 md:row-start-1" aria-hidden="true" />
      <div className="md:col-start-3 md:row-start-1">
        <Label>发送时间</Label>
      </div>
      {weekly ? (
        <div ref={fieldHeaderRef} className="md:col-start-4 md:row-start-1" style={fieldStyle}>
          <Label>周几</Label>
        </div>
      ) : monthly ? (
        <div ref={fieldHeaderRef} className="md:col-start-4 md:row-start-1" style={fieldStyle}>
          <Label>几号（1–28）</Label>
        </div>
      ) : <div aria-hidden="true" />}
      <div className="h-10 flex items-center text-xs text-gray-400 dark:text-gray-500 md:col-start-1 md:row-start-2">{hint}</div>
      <div className="h-10 flex items-center md:col-start-2 md:row-start-2">
        <Toggle checked={sched.enabled} onChange={(v) => onChange({ enabled: v })} label="启用" />
      </div>
      <div className="md:col-start-3 md:row-start-2">
        <input type="time" value={sched.time} onChange={(e) => onChange({ time: e.target.value })} className={compactInputCls} style={{ width: 'max-content' }} />
      </div>
      {weekly ? (
        <div ref={fieldControlRef} className="md:col-start-4 md:row-start-2" style={fieldStyle}>
          <select value={sched.weekday ?? 0} onChange={(e) => onChange({ weekday: Number(e.target.value) })} className={compactInputCls} style={controlStyle}>
            {WEEKDAYS.map((w, i) => <option key={w} value={i}>{w}</option>)}
          </select>
        </div>
      ) : monthly ? (
        <div ref={fieldControlRef} className="md:col-start-4 md:row-start-2" style={fieldStyle}>
          <input type="number" min={1} max={28} step={1} value={sched.day ?? 1} onChange={(e) => onChange({ day: clampReportDay(e.target.value) })} className={`${compactInputCls} report-day-input`} style={controlStyle} />
        </div>
      ) : <div className="md:col-start-4 md:row-start-2" aria-hidden="true" />}
    </div>
  )
}
