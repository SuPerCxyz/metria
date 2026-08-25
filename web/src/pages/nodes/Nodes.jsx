// 节点列表：节点名称/状态/Agent数/会话数/Token/费用/流量/最后上报。
// 支持前端添加节点（生成安装命令）、编辑与删除。

import React, { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DataTable from '../../components/tables/DataTable'
import StatusBadge from '../../components/common/StatusBadge'
import FilterBar from '../../components/filters/FilterBar'
import { ErrorState, LoadingSkeleton } from '../../components/feedback/Feedback'
import { api, q, rangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { fmtDateTime, fmtTokensShort, fmtUsd, fmtBytes, fmtPct100, fmtRelative, sumTokens, cacheHitRate } from '../../services/format'

export default function Nodes() {
  const { range } = useTimeRange()
  const navigate = useNavigate()
  const params = rangeParams(range)
  const [search, setSearch] = useState('')
  const [showCreate, setShowCreate] = useState(false)
  const [created, setCreated] = useState(null) // { node_id, name, token, hub_url, docker_command, native_command }
  const [editing, setEditing] = useState(null)
  const [confirmDelete, setConfirmDelete] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const installPath = (id) => `/nodes/${encodeURIComponent(id)}/install?hub_url=${encodeURIComponent(window.location.origin)}`

  const query = useQuery('nodes-list', () => api('/nodes'))
  const usage = useQuery(`nodes-usage${q({ ...params, dim: 'node' })}`, () => api(`/usage/breakdown${q({ ...params, dim: 'node' })}`))

  const usageMap = useMemo(() => {
    const m = new Map()
    for (const row of usage.data?.by || []) m.set(row.dimension, row)
    return m
  }, [usage.data])

  const filtered = useMemo(() => {
    const list = query.data?.nodes || []
    if (!search) return list
    const s = search.toLowerCase()
    return list.filter((x) => (x.name || '').toLowerCase().includes(s) || (x.id || '').toLowerCase().includes(s))
  }, [query.data, search])

  const refresh = () => {
    query.refresh()
    usage.refresh()
  }

  const closeCreate = () => {
    setShowCreate(false)
    setCreated(null)
    setError(null)
  }

  if (query.error) return <ErrorState error={query.error} onRetry={query.refresh} />
  if (query.loading) return <LoadingSkeleton rows={6} />

  const columns = [
    { key: 'name', label: '节点名称', sortable: true, render: (r) => r.name || r.id },
    { key: 'status', label: '状态', render: (r) => <StatusBadge status={r.status} /> },
    { key: 'collector_count', label: 'Agent 数量', render: (r) => String(r.collector_count ?? r.detected_clients ?? '—') },
    { key: 'sessions', label: '会话数', render: (r) => String(usageMap.get(r.id)?.sessions ?? '—') },
    { key: 'tokens', label: 'Token', render: (r) => fmtTokensShort(sumTokens(usageMap.get(r.id))) },
    { key: 'cache', label: '缓存命中率', render: (r) => cacheHitRate(usageMap.get(r.id)) != null ? fmtPct100(cacheHitRate(usageMap.get(r.id))) : '—' },
    { key: 'cost', label: '费用', render: (r) => fmtUsd(usageMap.get(r.id)?.calculated_cost_micro_usd) },
    { key: 'traffic', label: '网络流量', render: (r) => fmtBytes(usageMap.get(r.id)?.estimated_traffic_bytes) },
    {
      key: 'last_seen_at', label: '最后上报', sortable: true, render: (r) => <span title={fmtDateTime(r.last_seen_at)}>{fmtRelative(r.last_seen_at)}</span>,
    },
    {
      key: 'actions', label: '操作', render: (r) => (
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={(e) => { e.stopPropagation(); openInstall(r) }}
            className="text-xs text-indigo-600 dark:text-indigo-400 hover:underline"
          >
            安装
          </button>
          <button
            type="button"
            onClick={(e) => { e.stopPropagation(); setEditing(r) }}
            className="text-xs text-gray-600 dark:text-gray-300 hover:underline"
          >
            编辑
          </button>
          <button
            type="button"
            onClick={(e) => { e.stopPropagation(); setConfirmDelete(r) }}
            className="text-xs text-red-600 dark:text-red-400 hover:underline"
          >
            删除
          </button>
        </div>
      ),
    },
  ]

  return (
    <>
      <PageHeader
        title="节点"
        subtitle="监控节点与采集器状态"
      />
      <FilterBar
        searchPlaceholder="搜索节点名称…"
        onSearch={setSearch}
        actions={
          <button
            type="button"
            onClick={() => setShowCreate(true)}
            className="whitespace-nowrap text-sm text-white bg-indigo-600 hover:bg-indigo-700 rounded-lg px-3 py-2"
          >
            + 添加节点
          </button>
        }
      />
      <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-4">
        <DataTable columns={columns} data={filtered} pageSize={12} onRowClick={(r) => navigate(`/nodes/${encodeURIComponent(r.id)}`)} />
      </div>

      {showCreate && <CreateNodeDialog onClose={closeCreate} onCreated={handleCreated} busy={busy} setBusy={setBusy} error={error} setError={setError} />}
      {created && <InstallDialog data={created} onClose={closeCreate} />}
      {editing && <EditNodeDialog node={editing} onClose={() => setEditing(null)} onSaved={handleSaved} busy={busy} setBusy={setBusy} error={error} setError={setError} />}
      {confirmDelete && (
        <DeleteDialog
          node={confirmDelete}
          onClose={() => setConfirmDelete(null)}
          onDeleted={handleDeleted}
          busy={busy}
          setBusy={setBusy}
          error={error}
          setError={setError}
        />
      )}
    </>
  )

  function openInstall(r) {
    setBusy(true)
    setError(null)
    api(installPath(r.id))
      .then((data) => {
        setCreated({ node_id: data.node_id, name: data.name, token: data.token, hub_url: data.hub_url, agent_url: data.agent_url, mode: data.mode, platform: data.platform, architecture: data.architecture, agent_asset: data.agent_asset, docker_command: data.docker_command, native_command: data.native_command })
      })
      .catch((e) => setError(e.message))
      .finally(() => setBusy(false))
  }

  function handleCreated(data) {
    // 创建响应仅含一次性 token；命令由 install 端点生成（内含新签发的 active token）。
    setBusy(true)
    setError(null)
    api(installPath(data.node_id))
      .then((inst) => {
        setCreated({ ...data, ...inst })
        setShowCreate(false)
        refresh()
      })
      .catch((e) => setError(e.message))
      .finally(() => setBusy(false))
  }

  function handleSaved() {
    setEditing(null)
    refresh()
  }

  function handleDeleted() {
    setConfirmDelete(null)
    refresh()
  }
}

// ---------- 表单控件 ----------

function Label({ children }) {
  return <label className="block text-sm font-medium text-gray-600 dark:text-gray-300 mb-1">{children}</label>
}

function TextInput(props) {
  return (
    <input
      {...props}
      className="w-full text-sm rounded-lg border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-900 px-3 py-2 text-gray-800 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-indigo-500/40"
    />
  )
}

function SelectInput(props) {
  return (
    <select
      {...props}
      className="w-full text-sm rounded-lg border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-900 px-3 py-2 text-gray-800 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-indigo-500/40"
    />
  )
}

function DialogShell({ title, onClose, children }) {
  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-gray-900/50 backdrop-blur-sm px-4 py-8">
      <div className="w-full max-w-2xl bg-white dark:bg-gray-800 rounded-2xl border border-gray-200 dark:border-gray-700/60 shadow-xl">
        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-200 dark:border-gray-700/60">
          <h3 className="text-lg font-bold text-gray-800 dark:text-gray-100">{title}</h3>
          <button type="button" onClick={onClose} className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 text-2xl leading-none" aria-label="关闭">
            ×
          </button>
        </div>
        <div className="px-6 py-5">{children}</div>
      </div>
    </div>
  )
}

function FormError({ error }) {
  if (!error) return null
  return <div className="mt-3 text-sm text-red-600 dark:text-red-400">{error}</div>
}

// ---------- 添加节点 ----------

function CreateNodeDialog({ onClose, onCreated, busy, setBusy, error, setError }) {
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [labels, setLabels] = useState('')
  const [nodeIp, setNodeIp] = useState('')
  const [hubUrl, setHubUrl] = useState(window.location.origin)
  const [agentUrl, setAgentUrl] = useState('')
  const [platform, setPlatform] = useState('linux')
  const [architecture, setArchitecture] = useState('amd64')

  const submit = (e) => {
    e.preventDefault()
    if (!name.trim()) return
    if (!nodeIp.trim()) return
    setBusy(true)
    setError(null)
    api('/nodes', {
      method: 'POST',
      body: JSON.stringify({
        name: name.trim(),
        ip: nodeIp.trim(),
        description: description.trim() || undefined,
        labels: labels.split(/[,，\s]+/).map((s) => s.trim()).filter(Boolean),
        hub_url: hubUrl.trim() || undefined,
        agent_url: agentUrl.trim() || undefined,
        platform,
        architecture,
      }),
    })
      .then((data) => onCreated(data))
      .catch((err) => setError(err.message))
      .finally(() => setBusy(false))
  }

  return (
    <DialogShell title="添加节点" onClose={onClose}>
      <form onSubmit={submit}>
        <div className="space-y-4">
          <div>
            <Label>节点名称 *</Label>
            <TextInput value={name} onChange={(e) => setName(e.target.value)} placeholder="例如：prod-node-01" required autoFocus />
          </div>
          <div>
            <Label>节点 IP *</Label>
            <TextInput value={nodeIp} onChange={(e) => setNodeIp(e.target.value)} placeholder="例如：203.0.113.10 或 agent.example.com" required />
            <p className="mt-1 text-xs text-gray-400 dark:text-gray-500">节点所在公网主机的 IP 或可达主机名；Hub 在内网时，安装命令将用它生成公网 Agent 可连接的地址。</p>
          </div>
          <div>
            <Label>描述</Label>
            <TextInput value={description} onChange={(e) => setDescription(e.target.value)} placeholder="可选：节点用途说明" />
          </div>
          <div>
            <Label>标签</Label>
            <TextInput value={labels} onChange={(e) => setLabels(e.target.value)} placeholder="可选：用逗号分隔，如 生产, GPU" />
          </div>
          <div>
            <Label>Hub 地址</Label>
            <TextInput value={hubUrl} onChange={(e) => setHubUrl(e.target.value)} placeholder="http://hub-host:8080" />
            <p className="mt-1 text-xs text-gray-400 dark:text-gray-500">默认使用当前前端访问地址，安装命令会动态带入该地址；目标机必须能访问。</p>
          </div>
          <div>
            <Label>Agent 地址（Pull 模式，可选）</Label>
            <TextInput value={agentUrl} onChange={(e) => setAgentUrl(e.target.value)} placeholder="http://203.0.113.10:8090" />
            <p className="mt-1 text-xs text-gray-400 dark:text-gray-500">填写后 Agent 以 Pull 模式运行：Hub 主动访问该地址拉取数据（适合 Hub 在内网、Agent 在公网）；安装命令将只包含 Token，无需 Hub 地址与 Node ID。留空则使用传统 Push 模式。</p>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <div>
              <Label>Agent 平台 *</Label>
              <SelectInput value={platform} onChange={(e) => { setPlatform(e.target.value); if (e.target.value === 'windows') setArchitecture('amd64') }}>
                <option value="linux">Linux</option>
                <option value="windows">Windows</option>
              </SelectInput>
            </div>
            <div>
              <Label>Agent 架构 *</Label>
              <SelectInput value={architecture} onChange={(e) => setArchitecture(e.target.value)}>
                <option value="amd64">amd64 / x86_64</option>
                {platform === 'linux' && <option value="arm64">arm64 / aarch64</option>}
              </SelectInput>
            </div>
          </div>
        </div>
        <FormError error={error} />
        <div className="mt-5 flex justify-end gap-2">
          <button type="button" onClick={onClose} className="text-sm px-4 py-2 rounded-lg border border-gray-200 dark:border-gray-600 text-gray-600 dark:text-gray-300">
            取消
          </button>
          <button type="submit" disabled={busy} className="text-sm px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-700 text-white disabled:opacity-50">
            {busy ? '创建中…' : '创建节点'}
          </button>
        </div>
      </form>
    </DialogShell>
  )
}

// ---------- 安装命令展示 ----------

function InstallDialog({ data, onClose }) {
  const [tab, setTab] = useState(data.platform === 'windows' ? 'native' : 'docker')
  const [copied, setCopied] = useState(false)
  const command = tab === 'docker' ? data.docker_command : data.native_command
  const tabs = data.platform === 'windows'
    ? [{ key: 'native', label: 'PowerShell 安装' }]
    : [{ key: 'docker', label: 'Docker 安装' }, { key: 'native', label: '原生安装' }]

  const copy = () => {
    const done = () => {
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    }
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(command).then(done).catch(() => fallbackCopy())
    } else {
      fallbackCopy()
    }
    function fallbackCopy() {
      try {
        const ta = document.createElement('textarea')
        ta.value = command
        ta.style.position = 'fixed'
        ta.style.opacity = '0'
        document.body.appendChild(ta)
        ta.select()
        document.execCommand('copy')
        document.body.removeChild(ta)
        done()
      } catch {
        setCopied(false)
      }
    }
  }

  return (
    <DialogShell title={`安装 Agent · ${data.name || data.node_id}`} onClose={onClose}>
      <div className="space-y-4">
        <div className="text-xs text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-400/10 rounded-lg px-3 py-2">
          节点名称：<strong>{data.name || data.node_id}</strong> · 目标：{data.platform || 'linux'} / {data.architecture || 'amd64'} · 专属 Token 仅本次展示，请立即复制保存。
        </div>
        <div className="flex items-center gap-2">
          {tabs.map((t) => (
            <button
              key={t.key}
              type="button"
              onClick={() => { setTab(t.key); setCopied(false) }}
              className={`text-sm px-3 py-1.5 rounded-lg ${tab === t.key ? 'bg-indigo-600 text-white' : 'bg-gray-100 dark:bg-gray-700/40 text-gray-600 dark:text-gray-300'}`}
            >
              {t.label}
            </button>
          ))}
          <button type="button" onClick={copy} className="ml-auto text-sm px-3 py-1.5 rounded-lg border border-gray-200 dark:border-gray-600 text-gray-600 dark:text-gray-300">
            {copied ? '已复制 ✓' : '复制命令'}
          </button>
        </div>
        <pre className="overflow-x-auto rounded-xl bg-gray-900 dark:bg-black/50 p-4 text-xs text-gray-100 font-mono leading-relaxed whitespace-pre">
          {command}
        </pre>
        {data.mode === 'pull' ? (
          <p className="text-xs text-gray-400 dark:text-gray-500">
            Pull 模式：Hub 将主动访问 Agent 的 <code className="font-mono">{data.agent_url || 'http://<本机>:8090'}</code>（请放行该端口）；命令无需 Hub 地址与 Node ID。二进制下载地址 <code className="font-mono">{data.hub_url}</code> 公开可用。
          </p>
        ) : (
          <p className="text-xs text-gray-400 dark:text-gray-500">
            目标机需可访问 <code className="font-mono">{data.hub_url}</code>；二进制下载公开，无需 Token，Agent 启动后仍需使用专属 Token 注册 Hub。
          </p>
        )}
      </div>
    </DialogShell>
  )
}

// ---------- 编辑节点 ----------

function EditNodeDialog({ node, onClose, onSaved, busy, setBusy, error, setError }) {
  const [name, setName] = useState(node.name || '')
  const [description, setDescription] = useState(node.description || '')
  const [labels, setLabels] = useState(parseLabels(node.labels))
  const [nodeIp, setNodeIp] = useState(node.ip || '')
  const [hubUrl, setHubUrl] = useState(node.hub_url || window.location.origin)
  const [agentUrl, setAgentUrl] = useState(node.agent_url || '')
  const [platform, setPlatform] = useState(node.platform || 'linux')
  const [architecture, setArchitecture] = useState(node.architecture === 'aarch64' ? 'arm64' : (node.architecture || 'amd64'))

  function parseLabels(v) {
    if (!v) return ''
    try {
      const arr = typeof v === 'string' ? JSON.parse(v) : v
      return Array.isArray(arr) ? arr.join(', ') : ''
    } catch {
      return ''
    }
  }

  const submit = (e) => {
    e.preventDefault()
    if (!name.trim()) return
    if (!nodeIp.trim()) return
    setBusy(true)
    setError(null)
    api(`/nodes/${encodeURIComponent(node.id)}`, {
      method: 'PUT',
      body: JSON.stringify({
        name: name.trim(),
        ip: nodeIp.trim(),
        description: description.trim() || undefined,
        labels: labels.split(/[,，\s]+/).map((s) => s.trim()).filter(Boolean),
        hub_url: hubUrl.trim() || undefined,
        agent_url: agentUrl.trim() || (node.agent_url ? '' : undefined),
        platform,
        architecture,
      }),
    })
      .then(() => onSaved())
      .catch((err) => setError(err.message))
      .finally(() => setBusy(false))
  }

  return (
    <DialogShell title={`编辑节点 · ${node.name || node.id}`} onClose={onClose}>
      <form onSubmit={submit}>
        <div className="space-y-4">
          <div>
            <Label>节点名称 *</Label>
            <TextInput value={name} onChange={(e) => setName(e.target.value)} required />
          </div>
          <div>
            <Label>节点 IP *</Label>
            <TextInput value={nodeIp} onChange={(e) => setNodeIp(e.target.value)} required />
          </div>
          <div>
            <Label>描述</Label>
            <TextInput value={description} onChange={(e) => setDescription(e.target.value)} />
          </div>
          <div>
            <Label>标签</Label>
            <TextInput value={labels} onChange={(e) => setLabels(e.target.value)} placeholder="用逗号分隔" />
          </div>
          <div>
            <Label>Hub 地址</Label>
            <TextInput value={hubUrl} onChange={(e) => setHubUrl(e.target.value)} />
          </div>
          <div>
            <Label>Agent 地址（Pull 模式）</Label>
            <TextInput value={agentUrl} onChange={(e) => setAgentUrl(e.target.value)} placeholder="http://203.0.113.10:8090" />
            <p className="mt-1 text-xs text-gray-400 dark:text-gray-500">填写后 Hub 主动拉取该节点（Pull 模式）；清空并保存则切回 Push 模式。</p>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <div>
              <Label>Agent 平台 *</Label>
              <SelectInput value={platform} onChange={(e) => { setPlatform(e.target.value); if (e.target.value === 'windows') setArchitecture('amd64') }}>
                <option value="linux">Linux</option>
                <option value="windows">Windows</option>
              </SelectInput>
            </div>
            <div>
              <Label>Agent 架构 *</Label>
              <SelectInput value={architecture} onChange={(e) => setArchitecture(e.target.value)}>
                <option value="amd64">amd64 / x86_64</option>
                {platform === 'linux' && <option value="arm64">arm64 / aarch64</option>}
              </SelectInput>
            </div>
          </div>
        </div>
        <FormError error={error} />
        <div className="mt-5 flex justify-end gap-2">
          <button type="button" onClick={onClose} className="text-sm px-4 py-2 rounded-lg border border-gray-200 dark:border-gray-600 text-gray-600 dark:text-gray-300">
            取消
          </button>
          <button type="submit" disabled={busy} className="text-sm px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-700 text-white disabled:opacity-50">
            {busy ? '保存中…' : '保存'}
          </button>
        </div>
      </form>
    </DialogShell>
  )
}

// ---------- 删除节点 ----------

function DeleteDialog({ node, onClose, onDeleted, busy, setBusy, error, setError }) {
  const doDelete = () => {
    setBusy(true)
    setError(null)
    api(`/nodes/${encodeURIComponent(node.id)}`, { method: 'DELETE' })
      .then(() => onDeleted())
      .catch((err) => setError(err.message))
      .finally(() => setBusy(false))
  }

  return (
    <DialogShell title="删除节点" onClose={onClose}>
      <p className="text-sm text-gray-600 dark:text-gray-300">
        确定删除节点 <strong className="text-red-600 dark:text-red-400">{node.name || node.id}</strong>？将移除该节点的采集器与令牌，历史用量数据保留。
      </p>
      <FormError error={error} />
      <div className="mt-5 flex justify-end gap-2">
        <button type="button" onClick={onClose} className="text-sm px-4 py-2 rounded-lg border border-gray-200 dark:border-gray-600 text-gray-600 dark:text-gray-300">
          取消
        </button>
        <button type="button" onClick={doDelete} disabled={busy} className="text-sm px-4 py-2 rounded-lg bg-red-600 hover:bg-red-700 text-white disabled:opacity-50">
          {busy ? '删除中…' : '确认删除'}
        </button>
      </div>
    </DialogShell>
  )
}
