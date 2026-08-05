import { useState } from 'preact/hooks'
import { api } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { useQuery } from '../hooks/useQuery'
import { fmtUsd } from '../lib/format'

export function Pricing() {
  const catalogs = useQuery<any>('/pricing/catalogs', () => api('/pricing/catalogs'))
  const rules = useQuery<any>('/pricing/rules', () => api('/pricing/rules'))

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
      <h2>价格</h2>
      <p class="page-note">内置价格为近似参考；OpenRouter 等第三方目录在后续版本同步。</p>

      <Card title="价格目录">
        {(catalogs.data?.catalogs || []).length === 0 && <Empty text="暂无目录" />}
        <table class="table">
          <thead>
            <tr>
              <th>名称</th>
              <th>类型</th>
              <th>启用</th>
              <th>优先级</th>
              <th>最后成功</th>
            </tr>
          </thead>
          <tbody>
            {(catalogs.data?.catalogs || []).map((c: any) => (
              <tr key={c.id}>
                <td>{c.name}</td>
                <td>{c.kind}</td>
                <td>{c.enabled ? '是' : '否'}</td>
                <td>{c.priority}</td>
                <td>{c.last_success_at || '—'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>

      <div class="grid-2">
        <Card title="价格规则">
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

        <Card title="新增用户规则">
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

        <Card title="规则测试">
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
