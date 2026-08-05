import { useState } from 'preact/hooks'
import { api } from '../api/client'
import { Card, ErrorBox, Empty } from '../components/ui'
import { useQuery } from '../hooks/useQuery'
import { fmtBytes } from '../lib/format'

export function TrafficProfiles() {
  const profiles = useQuery<any>('/traffic/profiles', () => api('/traffic/profiles'))
  const [msg, setMsg] = useState('')
  const [form, setForm] = useState({
    client_pattern: 'claude-code',
    provider_pattern: '.*',
    model_pattern: 'claude-*',
    input_bytes_per_token: '3.6',
    output_bytes_per_token: '4.0',
    fixed_request_bytes: '1024',
    fixed_response_bytes: '128',
  })
  const set = (k: string) => (e: Event) => setForm({ ...form, [k]: (e.target as HTMLInputElement).value })

  const [test, setTest] = useState({ client: 'claude-code', model: 'claude-sonnet-4.5', input: '1000', output: '500' })
  const [testRes, setTestRes] = useState('')

  const create = async () => {
    setMsg('')
    try {
      await api('/traffic/profiles', {
        method: 'POST',
        body: JSON.stringify({ ...form, input_bytes_per_token: Number(form.input_bytes_per_token), output_bytes_per_token: Number(form.output_bytes_per_token), fixed_request_bytes: Number(form.fixed_request_bytes), fixed_response_bytes: Number(form.fixed_response_bytes) }),
      })
      setMsg('Profile 已创建')
      profiles.refresh()
    } catch (e) {
      setMsg(`失败：${(e as Error).message}`)
    }
  }

  const del = async (id: string) => {
    try {
      await api(`/traffic/profiles/${id}`, { method: 'DELETE' })
      profiles.refresh()
    } catch (e) {
      setMsg(`删除失败：${(e as Error).message}`)
    }
  }

  const learn = async () => {
    try {
      const r = await api<any>('/traffic/profiles/learn', { method: 'POST' })
      setMsg(`学习完成：生成 ${r.profiles_created} 个 profile`)
      profiles.refresh()
    } catch (e) {
      setMsg(`学习失败：${(e as Error).message}`)
    }
  }

  const reestimate = async () => {
    try {
      const r = await api<any>('/traffic/reestimate', { method: 'POST', body: JSON.stringify({}) })
      setMsg(`重新估算完成：${r.reestimated} 个调用（保留旧版本）`)
    } catch (e) {
      setMsg(`重新估算失败：${(e as Error).message}`)
    }
  }

  const runTest = async () => {
    try {
      const r = await api<any>('/traffic/profiles/test', {
        method: 'POST',
        body: JSON.stringify({ client: test.client, model: test.model, input_tokens: Number(test.input), output_tokens: Number(test.output) }),
      })
      setTestRes(
        `估算流量：${fmtBytes(r.estimated_total_wire_bytes)}（范围 ${fmtBytes(r.lower_bound_bytes)} ~ ${fmtBytes(r.upper_bound_bytes)}，可信度 ${Math.round((r.confidence ?? 0) * 100)}%，来源 ${r.estimation_source}）`,
      )
    } catch (e) {
      setTestRes(`测试失败：${(e as Error).message}`)
    }
  }

  if (profiles.error) return <ErrorBox error={profiles.error} onRetry={profiles.refresh} />

  return (
    <div class="page">
      <h2>Traffic Profiles</h2>
      <p class="page-note">版本化 bytes-per-token 配置；学习 profile 由样本聚合，用户 profile 可自定义。</p>
      {msg && <div class="state-box">{msg}</div>}

      <Card title="Profile 列表">
        <div class="dim-switch">
          <button type="button" class="btn small" onClick={learn}>
            聚合学习样本
          </button>
          <button type="button" class="btn small" onClick={reestimate}>
            历史重新估算
          </button>
        </div>
        {(profiles.data?.profiles || []).length === 0 && <Empty text="暂无 profile" />}
        <table class="table">
          <thead>
            <tr>
              <th>来源</th>
              <th>Client</th>
              <th>模型</th>
              <th>方向</th>
              <th>Input BPT</th>
              <th>Output BPT</th>
              <th>样本</th>
              <th>置信度</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody>
            {(profiles.data?.profiles || []).map((p: any) => (
              <tr key={p.id}>
                <td>{p.source}</td>
                <td>{p.client_pattern}</td>
                <td>{p.model_pattern}</td>
                <td>{p.direction}</td>
                <td>{p.input_bytes_per_token_p50?.toFixed(2)}</td>
                <td>{p.output_bytes_per_token_p50?.toFixed(2)}</td>
                <td>{p.sample_count}</td>
                <td>{p.confidence?.toFixed(2)}</td>
                <td>
                  {p.source === 'user' && (
                    <button type="button" class="btn small" onClick={() => del(p.id)}>
                      删除
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>

      <div class="grid-2">
        <Card title="新增用户 Profile">
          <div class="form-grid">
            <label>
              Client 匹配
              <input value={form.client_pattern} onInput={set('client_pattern')} />
            </label>
            <label>
              Provider 匹配
              <input value={form.provider_pattern} onInput={set('provider_pattern')} />
            </label>
            <label>
              模型匹配
              <input value={form.model_pattern} onInput={set('model_pattern')} />
            </label>
            <label>
              Input bytes/token
              <input type="number" step="0.1" value={form.input_bytes_per_token} onInput={set('input_bytes_per_token')} />
            </label>
            <label>
              Output bytes/token
              <input type="number" step="0.1" value={form.output_bytes_per_token} onInput={set('output_bytes_per_token')} />
            </label>
            <label>
              固定请求字节
              <input type="number" value={form.fixed_request_bytes} onInput={set('fixed_request_bytes')} />
            </label>
            <label>
              固定响应字节
              <input type="number" value={form.fixed_response_bytes} onInput={set('fixed_response_bytes')} />
            </label>
          </div>
          <button type="button" class="btn primary" onClick={create}>
            创建
          </button>
        </Card>

        <Card title="匹配测试">
          <div class="form-grid">
            <label>
              Client
              <input value={test.client} onInput={(e) => setTest({ ...test, client: (e.target as HTMLInputElement).value })} />
            </label>
            <label>
              模型
              <input value={test.model} onInput={(e) => setTest({ ...test, model: (e.target as HTMLInputElement).value })} />
            </label>
            <label>
              Input Tokens
              <input type="number" value={test.input} onInput={(e) => setTest({ ...test, input: (e.target as HTMLInputElement).value })} />
            </label>
            <label>
              Output Tokens
              <input type="number" value={test.output} onInput={(e) => setTest({ ...test, output: (e.target as HTMLInputElement).value })} />
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
