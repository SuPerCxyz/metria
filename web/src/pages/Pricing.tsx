import { useState } from 'preact/hooks'
import { api } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { useQuery } from '../hooks/useQuery'
import { fmtUsd } from '../lib/format'
import { t } from '../lib/i18n'

export function Pricing() {
  const catalogs = useQuery<any>('/pricing/catalogs', () => api('/pricing/catalogs'))
  const snapshots = useQuery<any>('/pricing/snapshots', () => api('/pricing/snapshots'))
  const rules = useQuery<any>('/pricing/rules', () => api('/pricing/rules'))
  const [msg, setMsg] = useState('')

  const refreshCatalog = async (id: string) => {
    try {
      const r = await api<any>(`/pricing/catalogs/${id}/refresh`, { method: 'POST' })
      setMsg(r.fetched ? `已同步：${r.rules} 条规则` : '未变化（ETag 未修改）')
      catalogs.refresh()
      snapshots.refresh()
    } catch (e) {
      setMsg(`同步失败：${(e as Error).message}（继续使用旧快照）`)
    }
  }

  const reprice = async () => {
    try {
      const r = await api<any>('/pricing/reprice', { method: 'POST', body: JSON.stringify({}) })
      setMsg(`重新计价完成：${r.repriced} 条（保留历史版本）`)
    } catch (e) {
      setMsg(`重新计价失败：${(e as Error).message}`)
    }
  }

  // 新建规则表单
  const [form, setForm] = useState({
    provider_pattern: '',
    model_pattern: 'claude-*',
    client_pattern: '*',
    input_price: '3000000',
    output_price: '15000000',
    cache_read_price: '',
    cache_write_price: '',
    reasoning_price: '',
    request_price: '',
    priority: '10',
  })
  const [saved, setSaved] = useState('')
  const [testModel, setTestModel] = useState('claude-sonnet-4.5')
  const [testIn, setTestIn] = useState('1000')
  const [testOut, setTestOut] = useState('500')
  const [testRes, setTestRes] = useState('')

  const set = (k: string) => (e: Event) => setForm({ ...form, [k]: (e.target as HTMLInputElement).value })

  const submitRule = async () => {
    setSaved('')
    const payload: Record<string, unknown> = {
      provider_pattern: form.provider_pattern || '.*',
      model_pattern: form.model_pattern || '.*',
      client_pattern: form.client_pattern || '*',
      priority: Number(form.priority || 0),
    }
    for (const k of ['input_price', 'output_price', 'cache_read_price', 'cache_write_price', 'reasoning_price', 'request_price']) {
      const v = (form as any)[k]
      if (v !== '') payload[k] = Number(v)
    }
    try {
      await api('/pricing/rules', { method: 'POST', body: JSON.stringify(payload) })
      setSaved('规则已保存')
      rules.refresh()
    } catch (e) {
      setSaved(`保存失败：${(e as Error).message}`)
    }
  }

  const runTest = async () => {
    try {
      const res = await api<any>('/pricing/test', {
        method: 'POST',
        body: JSON.stringify({
          model: testModel,
          provider: undefined,
          input_tokens: Number(testIn),
          output_tokens: Number(testOut),
        }),
      })
      setTestRes(
        res.pricing_available
          ? `计算费用：${fmtUsd(res.calculated_micro_usd ?? res.estimated_micro_usd)}（rule=${res.rule_id || '内置'}）`
          : '无可用价格（unavailable，不会硬造）',
      )
    } catch (e) {
      setTestRes(`测试失败：${(e as Error).message}`)
    }
  }

  if (catalogs.error) return <ErrorBox error={catalogs.error} onRetry={catalogs.refresh} />

  return (
    <div class="page">
      <h2>{t('nav.pricing')}</h2>
      <p class="page-note">内置价格为近似参考；OpenRouter 等第三方目录在后续版本同步。</p>

      {msg && <div class="state-box">{msg}</div>}

      <Card title={t('pricing.catalogs')}>
        <p class="page-note">
          OpenRouter 价格标记渠道 openrouter（非厂商直连）；LiteLLM 为第三方维护数据，可能存在延迟或误差。
        </p>
        {(catalogs.data?.catalogs || []).length === 0 && <Empty text={t('catalog.empty')} />}
        <table class="table">
          <thead>
            <tr>
              <th>名称</th>
              <th>类型</th>
              <th>优先级</th>
              <th>最后同步</th>
              <th>错误</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody>
            {(catalogs.data?.catalogs || []).map((c: any) => (
              <tr key={c.id}>
                <td>{c.name}</td>
                <td>{c.kind}</td>
                <td>{c.priority}</td>
                <td>{c.last_success_at || '—'}</td>
                <td>{(c.last_error || '').slice(0, 40) || '—'}</td>
                <td>
                  {c.kind !== 'builtin' && (
                    <button type="button" class="btn small" onClick={() => refreshCatalog(c.id)}>
                      刷新
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>

      <Card title={t('pricing.snapshots')}>
        {(snapshots.data?.snapshots || []).length === 0 && <Empty text={t('snapshots.empty')} />}
        <table class="table">
          <thead>
            <tr>
              <th>目录</th>
              <th>获取时间</th>
              <th>记录数</th>
              <th>ETag</th>
            </tr>
          </thead>
          <tbody>
            {(snapshots.data?.snapshots || []).slice(0, 20).map((s: any) => (
              <tr key={s.id}>
                <td>{s.catalog_id}</td>
                <td>{s.fetched_at}</td>
                <td>{s.record_count}</td>
                <td class="mono">{(s.etag || '—').slice(0, 20)}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <div class="dim-switch" style="margin-top:8px">
          <button type="button" class="btn small" onClick={reprice}>
            历史重新计价
          </button>
        </div>
      </Card>

      <div class="grid-2">
        <Card title={t('pricing.rules')}>
          <table class="table">
            <thead>
              <tr>
                <th>模型</th>
                <th>Provider</th>
                <th>Input/百万</th>
                <th>Output/百万</th>
                <th>优先级</th>
              </tr>
            </thead>
            <tbody>
              {(rules.data?.rules || []).map((r: any) => (
                <tr key={r.id}>
                  <td>{r.model_pattern}</td>
                  <td>{r.provider_pattern}</td>
                  <td>{fmtUsd(r.input_price)}</td>
                  <td>{fmtUsd(r.output_price)}</td>
                  <td>{r.priority}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>

        <Card title={t('pricing.newUserRule')}>
          <div class="form-grid">
            <label>
              Provider 匹配
              <input value={form.provider_pattern} onInput={set('provider_pattern')} placeholder=".*" />
            </label>
            <label>
              模型匹配
              <input value={form.model_pattern} onInput={set('model_pattern')} />
            </label>
            <label>
              Client 匹配
              <input value={form.client_pattern} onInput={set('client_pattern')} />
            </label>
            <label>
              Input 价格（微美元/百万 token）
              <input type="number" value={form.input_price} onInput={set('input_price')} />
            </label>
            <label>
              Output 价格
              <input type="number" value={form.output_price} onInput={set('output_price')} />
            </label>
            <label>
              Cache Read 价格
              <input type="number" value={form.cache_read_price} onInput={set('cache_read_price')} />
            </label>
            <label>
              Cache Write 价格
              <input type="number" value={form.cache_write_price} onInput={set('cache_write_price')} />
            </label>
            <label>
              优先级
              <input type="number" value={form.priority} onInput={set('priority')} />
            </label>
          </div>
          <button type="button" class="btn primary" onClick={submitRule}>
            保存规则
          </button>
          {saved && <div class="state-box">{saved}</div>}
        </Card>

        <Card title={t('pricing.test')}>
          <div class="form-grid">
            <label>
              模型
              <input value={testModel} onInput={(e) => setTestModel((e.target as HTMLInputElement).value)} />
            </label>
            <label>
              Input Tokens
              <input type="number" value={testIn} onInput={(e) => setTestIn((e.target as HTMLInputElement).value)} />
            </label>
            <label>
              Output Tokens
              <input type="number" value={testOut} onInput={(e) => setTestOut((e.target as HTMLInputElement).value)} />
            </label>
          </div>
          <button type="button" class="btn primary" onClick={runTest}>
            测试
          </button>
          {testRes && <div class="state-box">{testRes}</div>}
        </Card>
      </div>
    </div>
  )
}
