// 设置页：模型价格 / 数据保留 / 节点接入 / 系统设置。

import React, { useEffect, useMemo, useRef, useState } from 'react'
import { Link, useSearchParams, useNavigate } from 'react-router-dom'
import PageHeader from '../../components/common/PageHeader'
import DataTable from '../../components/tables/DataTable'
import SortableList from '../../components/common/SortableList'
import Segmented from '../../components/ui/Segmented'
import UserAvatar from '../../components/common/UserAvatar'
import { ErrorState, LoadingSkeleton } from '../../components/feedback/Feedback'
import { useToast } from '../../components/feedback/Toast'
import { api, setToken } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { fmtDateTime, fmtUsd } from '../../services/format'
import { EMPTY_LINK_DRAFT, EMPTY_RULE_DRAFT, PRICE_FIELDS, RULE_SOURCE_FILTERS, filterPricingRules, pricingRuleKind, pricingRuleKindMeta, serializeRuleDraft } from '../../services/pricing'
import ReportSettings from './ReportSettings'

const TABS = ['模型价格', '数据保留', '节点接入', '用量报告', '系统设置']
const TAB_KEYS = { pricing: '模型价格', retention: '数据保留', nodes: '节点接入', report: '用量报告', system: '系统设置' }
const AVATAR_COLORS = ['indigo', 'emerald', 'amber', 'rose', 'sky', 'violet']

const KIND_LABEL = { openrouter: 'OpenRouter', litellm: 'LiteLLM', custom: '自定义', builtin: '内置' }
const PRICE_SOURCE_LABEL = {
  openrouter_catalog: 'OpenRouter',
  litellm_catalog: 'LiteLLM',
  custom_http_catalog: '自定义目录',
}

function ModelAutocomplete({
  id,
  value,
  options,
  getOptionValue,
  getOptionLabel = getOptionValue,
  loading,
  disabled,
  placeholder,
  loadingPlaceholder,
  emptyPlaceholder,
  onChange,
}) {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [highlightedIndex, setHighlightedIndex] = useState(-1)
  const containerRef = useRef(null)

  const filteredOptions = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase()
    if (!needle) return options
    return options.filter((option) => String(getOptionValue(option) || '').toLocaleLowerCase().includes(needle))
  }, [getOptionValue, options, query])

  useEffect(() => {
    const closeWhenOutside = (event) => {
      if (!containerRef.current?.contains(event.target)) {
        setOpen(false)
        setHighlightedIndex(-1)
      }
    }
    document.addEventListener('pointerdown', closeWhenOutside)
    return () => document.removeEventListener('pointerdown', closeWhenOutside)
  }, [])

  useEffect(() => {
    setHighlightedIndex((current) => {
      if (filteredOptions.length === 0) return -1
      if (current < 0) return 0
      return Math.min(current, filteredOptions.length - 1)
    })
  }, [filteredOptions.length])

  const openList = () => {
    if (disabled) return
    setOpen(true)
    setQuery('')
    setHighlightedIndex(options.length > 0 ? 0 : -1)
  }

  const selectOption = (option) => {
    onChange(getOptionValue(option))
    setQuery('')
    setOpen(false)
    setHighlightedIndex(-1)
  }

  const handleInputChange = (event) => {
    setQuery(event.target.value)
    onChange(event.target.value)
    setOpen(true)
    setHighlightedIndex(0)
  }

  const handleKeyDown = (event) => {
    if (event.key === 'Escape') {
      setOpen(false)
      setHighlightedIndex(-1)
      return
    }
    if (event.key === 'Tab') {
      setOpen(false)
      return
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      if (!open) {
        openList()
        return
      }
      if (filteredOptions.length === 0) return
      setHighlightedIndex((current) => {
        const next = event.key === 'ArrowDown' ? current + 1 : current - 1
        if (next < 0) return filteredOptions.length - 1
        return next >= filteredOptions.length ? 0 : next
      })
      return
    }
    if (event.key === 'Enter' && open && filteredOptions[highlightedIndex]) {
      event.preventDefault()
      selectOption(filteredOptions[highlightedIndex])
    }
  }

  const handleBlur = () => {
    window.setTimeout(() => {
      if (!containerRef.current?.contains(document.activeElement)) {
        setOpen(false)
        setHighlightedIndex(-1)
      }
    }, 0)
  }

  return (
    <div ref={containerRef} className="relative">
      <input
        id={id}
        required
        type="text"
        role="combobox"
        aria-autocomplete="list"
        aria-expanded={open}
        aria-controls={`${id}-options`}
        aria-activedescendant={open && highlightedIndex >= 0 ? `${id}-option-${highlightedIndex}` : undefined}
        autoComplete="off"
        disabled={disabled}
        value={value}
        onChange={handleInputChange}
        onFocus={openList}
        onBlur={handleBlur}
        onKeyDown={handleKeyDown}
        placeholder={loading ? loadingPlaceholder : placeholder}
        className="w-full text-sm pl-3 pr-9 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-gray-700 dark:text-gray-200 focus:ring-2 focus:ring-indigo-500/30 focus:border-indigo-500 disabled:opacity-50"
      />
      <button
        type="button"
        tabIndex={-1}
        disabled={disabled}
        aria-label="打开模型匹配选项"
        onMouseDown={(event) => event.preventDefault()}
        onClick={openList}
        className="absolute right-1 top-1/2 -translate-y-1/2 rounded p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 disabled:opacity-50"
      >
        <svg aria-hidden="true" className="h-4 w-4" viewBox="0 0 20 20" fill="currentColor">
          <path fillRule="evenodd" d="M5.23 7.21a.75.75 0 0 1 1.06.02L10 11.168l3.71-3.938a.75.75 0 1 1 1.08 1.04l-4.25 4.5a.75.75 0 0 1-1.08 0l-4.25-4.5a.75.75 0 0 1 .02-1.06Z" clipRule="evenodd" />
        </svg>
      </button>
      {open && (
        <div id={`${id}-options`} role="listbox" className="absolute z-30 mt-1 max-h-60 w-full overflow-auto rounded-lg border border-gray-200 bg-white py-1 text-sm shadow-lg dark:border-gray-600 dark:bg-gray-800">
          {filteredOptions.length > 0 ? filteredOptions.map((option, index) => {
            const optionValue = getOptionValue(option)
            return (
              <div
                id={`${id}-option-${index}`}
                key={optionValue}
                role="option"
                aria-selected={optionValue === value}
                onMouseDown={(event) => event.preventDefault()}
                onMouseEnter={() => setHighlightedIndex(index)}
                onClick={() => selectOption(option)}
                className={`cursor-pointer px-3 py-2 text-gray-700 dark:text-gray-200 ${index === highlightedIndex ? 'bg-indigo-50 text-indigo-700 dark:bg-indigo-900/40 dark:text-indigo-200' : 'hover:bg-gray-50 dark:hover:bg-gray-700/60'}`}
              >
                {getOptionLabel(option)}
              </div>
            )
          }) : (
            <div role="status" className="px-3 py-2 text-gray-500 dark:text-gray-400">
              {options.length === 0 ? emptyPlaceholder : '没有匹配的模型，请修改输入'}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

export default function Settings() {
  const navigate = useNavigate()
  const [searchParams, setSearchParams] = useSearchParams()
  const requestedTab = TAB_KEYS[searchParams.get('tab')] || '模型价格'
  const [tab, setTab] = useState(requestedTab)
  const rules = useQuery('pricing-rules', () => api('/pricing/rules'))
  const pricingOptions = useQuery('pricing-options', () => api('/pricing/options'))

  const [profile, setProfile] = useState(null)
  const [profileDraft, setProfileDraft] = useState({ display_name: '', avatar_text: '', avatar_color: 'indigo' })
  const [systemInfo, setSystemInfo] = useState(null)
  const [accountLoading, setAccountLoading] = useState(true)
  const [accountError, setAccountError] = useState('')
  const [profileSaving, setProfileSaving] = useState(false)
  const [passwordDraft, setPasswordDraft] = useState({ old_password: '', new_password: '', confirm_password: '' })
  const [passwordSaving, setPasswordSaving] = useState(false)

  // 价格目录（计费模板链接）配置
  const [catalogs, setCatalogs] = useState([])
  const [catalogsLoading, setCatalogsLoading] = useState(true)
  const [catalogsError, setCatalogsError] = useState(null)
  const [drafts, setDrafts] = useState({})
  const [busyId, setBusyId] = useState(null)
  const [repricing, setRepricing] = useState(false)
  const [priceSearch, setPriceSearch] = useState('')
  const [priceSourceFilter, setPriceSourceFilter] = useState('all')
  const [ruleDraft, setRuleDraft] = useState(EMPTY_RULE_DRAFT)
  const [ruleApplyMode, setRuleApplyMode] = useState('future')
  const [ruleSaving, setRuleSaving] = useState(false)
  const [ruleError, setRuleError] = useState('')
  const [linkDraft, setLinkDraft] = useState(EMPTY_LINK_DRAFT)
  const [linkApplyMode, setLinkApplyMode] = useState('future')
  const [linkSaving, setLinkSaving] = useState(false)
  const [linkError, setLinkError] = useState('')
  const { notify } = useToast()

  useEffect(() => {
    setTab(requestedTab)
  }, [requestedTab])

  useEffect(() => {
    Promise.all([api('/auth/me'), api('/system/info')])
      .then(([me, info]) => {
        setProfile(me)
        setProfileDraft({
          display_name: me.display_name || '',
          avatar_text: me.avatar_text || '',
          avatar_color: me.avatar_color || 'indigo',
        })
        setSystemInfo(info)
        setAccountError('')
      })
      .catch((e) => setAccountError(e.message))
      .finally(() => setAccountLoading(false))
  }, [])

  const selectTab = (next) => {
    setTab(next)
    const key = Object.entries(TAB_KEYS).find(([, value]) => value === next)?.[0]
    setSearchParams(key && key !== 'pricing' ? { tab: key } : {})
  }

  const saveProfile = async (event) => {
    event.preventDefault()
    setAccountError('')
    setProfileSaving(true)
    try {
      const updated = await api('/auth/profile', {
        method: 'PUT',
        body: JSON.stringify(profileDraft),
      })
      setProfile(updated)
      setProfileDraft({
        display_name: updated.display_name || '',
        avatar_text: updated.avatar_text || '',
        avatar_color: updated.avatar_color || 'indigo',
      })
      window.dispatchEvent(new CustomEvent('metria:profile-updated'))
      notify('资料已保存')
    } catch (e) {
      notify(`资料保存失败：${e.message}`, 'error')
    } finally {
      setProfileSaving(false)
    }
  }

  const changePassword = async (event) => {
    event.preventDefault()
    setAccountError('')
    if (passwordDraft.new_password !== passwordDraft.confirm_password) {
      notify('两次输入的新密码不一致', 'error')
      return
    }
    if (passwordDraft.new_password.length < 8) {
      notify('新密码至少需要 8 个字符', 'error')
      return
    }
    setPasswordSaving(true)
    try {
      await api('/auth/change-password', {
        method: 'POST',
        body: JSON.stringify({ old_password: passwordDraft.old_password, new_password: passwordDraft.new_password }),
      })
      notify('密码已修改，即将返回登录页…')
      setTimeout(() => {
        setToken(null)
        window.dispatchEvent(new CustomEvent('metria:unauth'))
        navigate('/login', { replace: true })
      }, 800)
    } catch (e) {
      notify(`密码修改失败：${e.message}`, 'error')
    } finally {
      setPasswordSaving(false)
    }
  }

  const priceRules = useMemo(() => {
    const query = priceSearch.trim().toLocaleLowerCase()
    const matched = filterPricingRules(rules.data?.rules || [], priceSourceFilter)
    const searched = query
      ? matched.filter((rule) => `${rule.model_pattern || ''} ${rule.price_equivalent_to || ''} ${rule.source || ''}`.toLocaleLowerCase().includes(query))
      : matched
    // 用户规则（价格关联 / 用户价格）优先展示，避免被大量目录规则淹没
    return [...searched].sort((left, right) => {
      const leftUser = pricingRuleKind(left) === 'link' || pricingRuleKind(left) === 'user'
      const rightUser = pricingRuleKind(right) === 'link' || pricingRuleKind(right) === 'user'
      if (leftUser !== rightUser) return leftUser ? -1 : 1
      return 0
    })
  }, [priceSearch, priceSourceFilter, rules.data])

  const priceColumns = useMemo(() => [
    { key: 'model_pattern', label: '模型匹配', sortable: true, render: (rule) => <span title={rule.model_pattern} className="block max-w-[32rem] truncate">{rule.model_pattern}</span> },
    { key: 'price_equivalent_to', label: '价格等价于', sortable: true, sortValue: (rule) => rule.price_equivalent_to, render: (rule) => rule.price_equivalent_to ? <span title={rule.price_equivalent_to} className="block max-w-[18rem] truncate">{rule.price_equivalent_to}{rule.price_equivalent_missing_as_free ? '（缺价按 0）' : ''}</span> : '—' },
    { key: 'input_price', label: '输入/百万', sortable: true, render: (rule) => <span className="tabular-nums">{rule.input_price != null ? fmtUsd(rule.input_price) : '—'}</span> },
    { key: 'output_price', label: '输出/百万', sortable: true, render: (rule) => <span className="tabular-nums">{rule.output_price != null ? fmtUsd(rule.output_price) : '—'}</span> },
    { key: 'source', label: '来源', sortable: true, sortValue: (rule) => pricingRuleKindMeta(rule).label, render: (rule) => { const meta = pricingRuleKindMeta(rule); return <span className={`text-xs px-2 py-0.5 rounded-full ${meta.className}`}>{meta.label}</span> } },
  ], [])

  const sourceModelOptions = pricingOptions.data?.used_models || []
  const catalogModelOptions = useMemo(() => {
    const byModel = new Map()
    for (const option of pricingOptions.data?.catalog_models || []) {
      const model = (option.model_pattern || '').trim()
      if (!model) continue
      const key = model.toLocaleLowerCase()
      const current = byModel.get(key)
      if (current) {
        current.price_available = current.price_available || Boolean(option.price_available)
        if (!current.sources.includes(option.source)) current.sources.push(option.source)
      } else {
        byModel.set(key, {
          model_pattern: model,
          price_available: Boolean(option.price_available),
          sources: [option.source],
        })
      }
    }
    return [...byModel.values()]
  }, [pricingOptions.data])

  const selectedSourceModel = useMemo(() => {
    const value = linkDraft.model_pattern.trim().toLocaleLowerCase()
    return sourceModelOptions.find((option) => option.model.toLocaleLowerCase() === value) || null
  }, [linkDraft.model_pattern, sourceModelOptions])

  const selectedCatalogModel = useMemo(() => {
    const value = linkDraft.price_equivalent_to.trim().toLocaleLowerCase()
    return catalogModelOptions.find((option) => option.model_pattern.toLocaleLowerCase() === value) || null
  }, [catalogModelOptions, linkDraft.price_equivalent_to])

  const loadCatalogs = async () => {
    setCatalogsLoading(true)
    try {
      const j = await api('/pricing/catalogs')
      setCatalogs(j.catalogs || [])
      setCatalogsError(null)
    } catch (e) {
      setCatalogsError(e.message)
    } finally {
      setCatalogsLoading(false)
    }
  }

  useEffect(() => { loadCatalogs() }, [])

  const draft = (c) => ({ base_url: c.base_url || '', authentication_type: c.authentication_type || '', enabled: !!c.enabled, ...(drafts[c.id] || {}) })
  const setDraft = (id, patch) => setDrafts((d) => ({ ...d, [id]: { ...(d[id] || {}), ...patch } }))

  const saveCatalog = async (c) => {
    const d = draft(c)
    setBusyId(c.id)
    try {
      await api(`/pricing/catalogs/${encodeURIComponent(c.id)}`, { method: 'PUT', body: JSON.stringify({ base_url: d.base_url.trim(), authentication_type: d.authentication_type.trim(), enabled: d.enabled }) })
      notify(`已保存 ${c.name}`)
      await loadCatalogs()
    } catch (e) {
      notify(`保存失败：${e.message}`, 'error')
    } finally {
      setBusyId(null)
    }
  }

  const syncCatalog = async (c) => {
    setBusyId(c.id)
    try {
      const j = await api(`/pricing/catalogs/${encodeURIComponent(c.id)}/refresh`, { method: 'POST' })
      notify(`同步完成：${j.fetched ? `更新 ${j.rules} 条规则` : '无变化'}` + (j.repriced != null ? `，重新计价 ${j.repriced} 条` : ''))
      await loadCatalogs()
      await pricingOptions.refresh()
    } catch (e) {
      notify(`同步失败：${e.message}`, 'error')
    } finally {
      setBusyId(null)
    }
  }

  const doReprice = async () => {
    setRepricing(true)
    try {
      const j = await api('/pricing/reprice', { method: 'POST', body: JSON.stringify({}) })
      notify(`重新计价完成：${j.repriced} 条调用`)
    } catch (e) {
      notify(`重新计价失败：${e.message}`, 'error')
    } finally {
      setRepricing(false)
    }
  }

  const savePricingRule = async (event, draft, applyMode, setDraft, setApplyMode, setSaving, setError, successMessage) => {
    event.preventDefault()
    setError('')
    let payload
    try {
      payload = serializeRuleDraft(draft, {
        effectiveFrom: applyMode === 'future' ? new Date().toISOString() : undefined,
      })
    } catch (e) {
      setError(e.message)
      notify(`价格规则校验失败：${e.message}`, 'error')
      return
    }

    setSaving(true)
    try {
      await api('/pricing/rules', { method: 'POST', body: JSON.stringify(payload) })
      setDraft(draft.price_equivalent_to ? EMPTY_LINK_DRAFT : EMPTY_RULE_DRAFT)
      setApplyMode('future')
      if (applyMode === 'all') {
        try {
          const repriced = await api('/pricing/reprice', { method: 'POST', body: JSON.stringify({}) })
          notify(`${successMessage}，重新计价 ${repriced.repriced ?? 0} 条调用`)
        } catch (e) {
          notify(`模型价格已保存，但历史重新计价失败：${e.message}`, 'error')
        }
      } else {
        notify(`${successMessage}，仅对后续数据生效`)
      }
      try {
        await rules.refresh()
      } catch (e) {
        notify(`价格规则列表刷新失败：${e.message}`, 'error')
      }
    } catch (e) {
      notify(`保存失败：${e.message}`, 'error')
    } finally {
      setSaving(false)
    }
  }

  const createRule = (event) => savePricingRule(event, ruleDraft, ruleApplyMode, setRuleDraft, setRuleApplyMode, setRuleSaving, setRuleError, '已保存模型价格')
  const createLinkRule = (event) => {
    event.preventDefault()
    if (!selectedSourceModel || !selectedCatalogModel) {
      const message = '请从平台模型和价格目录的匹配选项中各选择一个模型'
      setLinkError(message)
      notify(`价格关联校验失败：${message}`, 'error')
      return
    }
    savePricingRule(event, {
      ...linkDraft,
      model_pattern: selectedSourceModel.model,
      price_equivalent_to: selectedCatalogModel.model_pattern,
      provider_pattern: '*',
    }, linkApplyMode, setLinkDraft, setLinkApplyMode, setLinkSaving, setLinkError, '已保存价格关联')
  }

  return (
    <>
      <PageHeader title="设置" subtitle="价格配置与系统设置" />
      <div className="mb-4">
        <Segmented items={TABS} value={tab} onChange={selectTab} />
      </div>

      {tab === '模型价格' && (
        <>
          {/* 价格目录（计费模板链接） */}
          <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6 mb-6">
            <div className="flex items-center justify-between mb-1">
              <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">价格目录（计费模板链接）</h2>
              <button type="button" onClick={doReprice} disabled={repricing} className="text-xs text-white bg-indigo-600 hover:bg-indigo-700 rounded-lg px-3 py-1.5 disabled:opacity-50">
                {repricing ? '重新计价中…' : '重新计价'}
              </button>
            </div>
            <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">配置外部价格目录地址，点击「同步」拉取官方计费并重新计算费用。每个目录只保留最新快照与规则。</p>
            {catalogsError && <ErrorState error={catalogsError} onRetry={loadCatalogs} />}
            {catalogsLoading && <LoadingSkeleton rows={3} />}
            {!catalogsLoading && !catalogsError && (
              <SortableList
                items={catalogs}
                options={[
                  { key: 'name', label: '名称', getValue: (c) => c.name },
                  { key: 'kind', label: '类型', getValue: (c) => KIND_LABEL[c.kind] || c.kind },
                  { key: 'last_success_at', label: '最近同步', getValue: (c) => c.last_success_at },
                ]}
                className="flex flex-col gap-3"
                renderItem={(c) => {
                  const isBuiltin = c.kind === 'builtin'
                  const d = draft(c)
                  return (
                    <div key={c.id} className="border border-gray-200 dark:border-gray-700/60 rounded-xl p-4">
                      <div className="flex items-center justify-between gap-3 flex-wrap">
                        <div className="flex items-center gap-2 flex-wrap">
                          <span className="font-semibold text-gray-800 dark:text-gray-100">{c.name}</span>
                          <span className="text-xs px-2 py-0.5 rounded-full bg-gray-100 dark:bg-gray-700/40 text-gray-600 dark:text-gray-300">{KIND_LABEL[c.kind] || c.kind}</span>
                          <label className="inline-flex items-center gap-1.5 text-sm text-gray-600 dark:text-gray-300">
                            <input type="checkbox" checked={d.enabled} disabled={isBuiltin} onChange={(e) => setDraft(c.id, { enabled: e.target.checked })} />
                            启用
                          </label>
                        </div>
                        <div className="flex items-center gap-2">
                          {!isBuiltin && (
                            <button type="button" onClick={() => syncCatalog(c)} disabled={busyId === c.id || !d.enabled} className="text-xs text-gray-600 dark:text-gray-300 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-1.5 disabled:opacity-50">
                              同步
                            </button>
                          )}
                          {!isBuiltin && (
                            <button type="button" onClick={() => saveCatalog(c)} disabled={busyId === c.id} className="text-xs text-white bg-indigo-600 hover:bg-indigo-700 rounded-lg px-3 py-1.5 disabled:opacity-50">
                              保存
                            </button>
                          )}
                        </div>
                      </div>
                      {!isBuiltin && (
                        <>
                          <label className="block text-xs text-gray-400 dark:text-gray-500 mt-3 mb-1">计费模板链接（base_url）</label>
                          <input
                            type="text"
                            value={d.base_url}
                            onChange={(e) => setDraft(c.id, { base_url: e.target.value })}
                            placeholder="https://openrouter.ai/api/v1/models"
                            className="w-full text-sm px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-gray-700 dark:text-gray-200 focus:ring-2 focus:ring-indigo-500/30 focus:border-indigo-500"
                          />
                          <label className="block text-xs text-gray-400 dark:text-gray-500 mt-2 mb-1">认证 Token（可选）</label>
                          <input
                            type="password"
                            value={d.authentication_type}
                            onChange={(e) => setDraft(c.id, { authentication_type: e.target.value })}
                            placeholder="Bearer xxx（可选）"
                            className="w-full text-sm px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-gray-700 dark:text-gray-200 focus:ring-2 focus:ring-indigo-500/30 focus:border-indigo-500"
                          />
                        </>
                      )}
                      {c.last_error && <div className="text-xs text-red-500 dark:text-red-400 mt-2">上次同步失败：{c.last_error}</div>}
                      {c.last_success_at && !c.last_error && <div className="text-xs text-gray-400 dark:text-gray-500 mt-2">上次成功：<span className="tabular-nums whitespace-nowrap">{fmtDateTime(c.last_success_at, { second: '2-digit' })}</span></div>}
                    </div>
                  )
                }}
              />
            )}
          </div>

          <form onSubmit={createRule} className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6 mb-6">
            <div className="flex items-center justify-between gap-3 mb-1">
              <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">添加用户价格</h2>
              <span className="text-xs text-gray-400 dark:text-gray-500">单位：美元 / 百万 Token</span>
            </div>
            <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">
              可填写原始模型名，保存时会自动去掉 Provider 前缀和 <code className="text-xs bg-gray-100 dark:bg-gray-700 px-1.5 py-0.5 rounded">-free</code> 后缀；价格填写 0 表示免费。需要跟随其他模型价格时，请使用下方“添加价格关联”。
            </p>
            {ruleError && <div className="mb-4 text-sm text-red-600 dark:text-red-400">{ruleError}</div>}
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <label className="block">
                <span className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">模型名称或匹配模式</span>
                <input
                  required
                  type="text"
                  value={ruleDraft.model_pattern}
                  onChange={(e) => setRuleDraft((d) => ({ ...d, model_pattern: e.target.value }))}
                  placeholder="例如 mimo-v2.5 或 opencode-go/mimo-v2.5-free"
                  className="w-full text-sm px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-gray-700 dark:text-gray-200 focus:ring-2 focus:ring-indigo-500/30 focus:border-indigo-500"
                />
              </label>
              <label className="block">
                <span className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">Provider 匹配（可选）</span>
                <input
                  type="text"
                  value={ruleDraft.provider_pattern}
                  onChange={(e) => setRuleDraft((d) => ({ ...d, provider_pattern: e.target.value }))}
                  placeholder="留空匹配所有 Provider"
                  className="w-full text-sm px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-gray-700 dark:text-gray-200 focus:ring-2 focus:ring-indigo-500/30 focus:border-indigo-500"
                />
              </label>
              {PRICE_FIELDS.map(([key, label]) => (
                <label key={key} className="block">
                  <span className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{label}</span>
                  <input
                    type="text"
                    inputMode="numeric"
                    value={ruleDraft[key]}
                    onChange={(e) => setRuleDraft((d) => ({ ...d, [key]: e.target.value }))}
                    placeholder="例如 0.08；免费请填 0"
                    className="w-full text-sm px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-gray-700 dark:text-gray-200 focus:ring-2 focus:ring-indigo-500/30 focus:border-indigo-500"
                  />
                </label>
              ))}
            </div>
            <fieldset className="mt-4 rounded-lg border border-gray-200 dark:border-gray-700/60 p-3">
              <legend className="px-1 text-xs font-medium text-gray-500 dark:text-gray-400">生效范围</legend>
              <div className="flex flex-col gap-2 sm:flex-row sm:gap-5 text-sm text-gray-600 dark:text-gray-300">
                <label className="inline-flex items-center gap-2">
                  <input type="radio" name="price-apply-mode" value="future" checked={ruleApplyMode === 'future'} onChange={() => setRuleApplyMode('future')} />
                  仅后续数据生效
                </label>
                <label className="inline-flex items-center gap-2">
                  <input type="radio" name="price-apply-mode" value="all" checked={ruleApplyMode === 'all'} onChange={() => setRuleApplyMode('all')} />
                  强制重新计算全部历史
                </label>
              </div>
              <p className="mt-2 text-xs text-gray-400 dark:text-gray-500">{ruleApplyMode === 'all' ? '会更新历史费用并保留已有价格匹配记录。' : '保存时间以前的数据保持原费用，不触发历史重新计价。'}</p>
            </fieldset>
            <div className="mt-4 flex justify-end">
              <button type="submit" disabled={ruleSaving} className="text-sm text-white bg-indigo-600 hover:bg-indigo-700 rounded-lg px-4 py-2 disabled:opacity-50">
                {ruleSaving ? (ruleApplyMode === 'all' ? '保存并重新计价中…' : '保存中…') : (ruleApplyMode === 'all' ? '保存并重新计价全部历史' : '保存，仅后续生效')}
              </button>
            </div>
          </form>

          <form onSubmit={createLinkRule} className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6 mb-6">
            <div className="flex items-center justify-between gap-3 mb-1">
              <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">添加价格关联</h2>
              <span className="text-xs text-gray-400 dark:text-gray-500">跟随已有模型价格</span>
            </div>
            <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">
              从平台已使用模型中选择一个自定义模型，再从已同步的价格目录中选择等价模型；费用会按照目标模型的当前价格计算。
            </p>
            {linkError && <div className="mb-4 text-sm text-red-600 dark:text-red-400">{linkError}</div>}
            {pricingOptions.error && <div className="mb-4 text-sm text-red-600 dark:text-red-400">模型选项加载失败：{pricingOptions.error.message}</div>}
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="block">
                <label htmlFor="price-link-source-model" className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">平台已使用的模型</label>
                <ModelAutocomplete
                  id="price-link-source-model"
                  value={linkDraft.model_pattern}
                  options={sourceModelOptions}
                  getOptionValue={(option) => option.model}
                  getOptionLabel={(option) => `${option.model}（${option.calls} 次）`}
                  disabled={pricingOptions.loading || sourceModelOptions.length === 0}
                  loading={pricingOptions.loading}
                  loadingPlaceholder="正在加载平台模型…"
                  placeholder="输入或选择平台已使用的模型"
                  emptyPlaceholder="暂无平台已使用模型"
                  onChange={(value) => {
                    setLinkError('')
                    setLinkDraft((d) => ({ ...d, model_pattern: value, provider_pattern: '*' }))
                  }}
                />
              </div>
              <div className="block">
                <label htmlFor="price-link-target-model" className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">价格目录中的等价模型</label>
                <ModelAutocomplete
                  id="price-link-target-model"
                  value={linkDraft.price_equivalent_to}
                  options={catalogModelOptions}
                  getOptionValue={(option) => option.model_pattern}
                  getOptionLabel={(option) => `${option.model_pattern} · ${option.sources.map((source) => PRICE_SOURCE_LABEL[source] || source).join('、')}${!option.price_available ? ' · 当前无价格' : ''}`}
                  disabled={pricingOptions.loading || catalogModelOptions.length === 0}
                  loading={pricingOptions.loading}
                  loadingPlaceholder="正在加载价格目录模型…"
                  placeholder="输入或选择价格目录中的等价模型"
                  emptyPlaceholder="暂无可选价格目录模型"
                  onChange={(value) => {
                    setLinkError('')
                    setLinkDraft((d) => ({ ...d, price_equivalent_to: value }))
                  }}
                />
              </div>
              {!pricingOptions.loading && !pricingOptions.error && sourceModelOptions.length === 0 && <p className="md:col-span-2 text-xs text-amber-600 dark:text-amber-400">暂无平台已使用模型，采集到模型调用后才能建立价格关联。</p>}
              {!pricingOptions.loading && !pricingOptions.error && catalogModelOptions.length === 0 && <p className="md:col-span-2 text-xs text-amber-600 dark:text-amber-400">暂无可选价格目录模型，请先在上方同步价格目录。</p>}
              {linkDraft.price_equivalent_to.trim() && (
                <label className="inline-flex items-start gap-2 md:col-span-2 text-sm text-gray-600 dark:text-gray-300">
                  <input
                    type="checkbox"
                    checked={linkDraft.price_equivalent_missing_as_free}
                    onChange={(e) => setLinkDraft((d) => ({ ...d, price_equivalent_missing_as_free: e.target.checked }))}
                    className="mt-0.5"
                  />
                  <span>
                    <span className="block">目标模型无价格时按 0 计费</span>
                    <span className="block text-xs text-gray-400 dark:text-gray-500">这是明确的免费配置；目标有价格时仍使用目标价格。</span>
                  </span>
                </label>
              )}
            </div>
            <fieldset className="mt-4 rounded-lg border border-gray-200 dark:border-gray-700/60 p-3">
              <legend className="px-1 text-xs font-medium text-gray-500 dark:text-gray-400">生效范围</legend>
              <div className="flex flex-col gap-2 sm:flex-row sm:gap-5 text-sm text-gray-600 dark:text-gray-300">
                <label className="inline-flex items-center gap-2">
                  <input type="radio" name="price-link-apply-mode" value="future" checked={linkApplyMode === 'future'} onChange={() => setLinkApplyMode('future')} />
                  仅后续数据生效
                </label>
                <label className="inline-flex items-center gap-2">
                  <input type="radio" name="price-link-apply-mode" value="all" checked={linkApplyMode === 'all'} onChange={() => setLinkApplyMode('all')} />
                  强制重新计算全部历史
                </label>
              </div>
              <p className="mt-2 text-xs text-gray-400 dark:text-gray-500">{linkApplyMode === 'all' ? '会更新历史费用并保留已有价格匹配记录。' : '保存时间以前的数据保持原费用，不触发历史重新计价。'}</p>
            </fieldset>
            <div className="mt-4 flex justify-end">
              <button type="submit" disabled={linkSaving || pricingOptions.loading || !selectedSourceModel || !selectedCatalogModel} className="text-sm text-white bg-indigo-600 hover:bg-indigo-700 rounded-lg px-4 py-2 disabled:opacity-50">
                {linkSaving ? (linkApplyMode === 'all' ? '保存关联并重新计价中…' : '保存关联中…') : (linkApplyMode === 'all' ? '保存关联并重新计价全部历史' : '保存价格关联')}
              </button>
            </div>
          </form>

          {/* 价格规则 */}
          <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
            <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">价格规则</h2>
              <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
                <Segmented items={RULE_SOURCE_FILTERS} value={priceSourceFilter} onChange={setPriceSourceFilter} />
                <label className="relative block w-full sm:w-80">
                  <span className="sr-only">搜索价格规则</span>
                  <svg aria-hidden="true" className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <circle cx="11" cy="11" r="8" />
                    <path d="m21 21-4.3-4.3" />
                  </svg>
                  <input
                    type="search"
                    value={priceSearch}
                    onChange={(event) => setPriceSearch(event.target.value)}
                    placeholder="搜索模型或来源"
                    className="w-full rounded-lg border border-gray-300 bg-white py-2 pl-9 pr-3 text-sm text-gray-700 focus:border-indigo-500 focus:ring-2 focus:ring-indigo-500/30 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-200"
                  />
                </label>
              </div>
            </div>
            {rules.error && <ErrorState error={rules.error} onRetry={rules.refresh} />}
            {rules.loading && <LoadingSkeleton rows={5} />}
            {!rules.loading && !rules.error && (
              <DataTable columns={priceColumns} data={priceRules} pageSize={20} emptyText={priceSearch || priceSourceFilter !== 'all' ? '没有匹配的价格规则' : '暂无价格规则'} />
            )}
          </div>
        </>
      )}

      {tab === '数据保留' && (
        <RetentionSettings loading={accountLoading} error={accountError} info={systemInfo} />
      )}

      {tab === '节点接入' && (
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-2">节点接入</h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">节点创建、安装命令和接入状态统一在节点管理页面完成。</p>
          <Link to="/nodes" className="inline-flex items-center rounded-lg bg-indigo-600 px-3 py-2 text-sm text-white hover:bg-indigo-700">
            前往节点管理
          </Link>
          <p className="mt-3 text-xs text-gray-400 dark:text-gray-500">进入节点管理后，点击“添加节点”即可获取 Docker 或原生 Agent 安装命令。</p>
        </div>
      )}

      {tab === '用量报告' && <ReportSettings />}

      {tab === '系统设置' && (
        <AccountSettings
          loading={accountLoading}
          error={accountError}
          profile={profile}
          authMode={systemInfo?.auth_mode}
          profileDraft={profileDraft}
          setProfileDraft={setProfileDraft}
          saveProfile={saveProfile}
          profileSaving={profileSaving}
          passwordDraft={passwordDraft}
          setPasswordDraft={setPasswordDraft}
          changePassword={changePassword}
          passwordSaving={passwordSaving}
          colors={AVATAR_COLORS}
        />
      )}
    </>
  )
}

function FieldLabel({ children }) {
  return <span className="mb-1 block text-xs font-medium text-gray-500 dark:text-gray-400">{children}</span>
}

function TextField({ value, onChange, placeholder, maxLength, type = 'text', readOnly = false }) {
  return (
    <input
      type={type}
      value={value}
      readOnly={readOnly}
      maxLength={maxLength}
      onChange={onChange}
      placeholder={placeholder}
      className="w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700 focus:border-indigo-500 focus:ring-2 focus:ring-indigo-500/30 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-200"
    />
  )
}

function AccountSettings({ loading, error, profile, authMode, profileDraft, setProfileDraft, saveProfile, profileSaving, passwordDraft, setPasswordDraft, changePassword, passwordSaving, colors }) {
  if (loading) return <LoadingSkeleton rows={4} />
  if (!profile) return <ErrorState error={error || '账户资料加载失败'} />
  const oidcManaged = authMode === 'oidc' || authMode === 'oidc+password'
  const oidcEmail = profile.email || (oidcManaged && profile.username.includes('@') ? profile.username : '')
  return (
    <div className="space-y-4">
      <form onSubmit={saveProfile} className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
        <div className="flex items-center justify-between gap-3 mb-5 flex-wrap">
          <div>
            <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">账户资料</h2>
            <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
              {oidcManaged ? '显示名称可在此修改；OIDC 头像会在登录时同步，文字和颜色用于图片不可用时回退。' : '修改顶部用户菜单中显示的名称和头像。'}
            </p>
          </div>
          <UserAvatar user={{ ...profile, ...profileDraft }} size="lg" />
        </div>
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          <div>
            <label>
              <FieldLabel>用户名</FieldLabel>
              <TextField value={profile.username} readOnly />
            </label>
            <span className="mt-1 block text-xs text-gray-400 dark:text-gray-500">
              {oidcManaged ? '用户名和邮箱由 OIDC 身份提供商管理，不能在此修改。' : '单 Admin 账户，用户名不可修改。'}
            </span>
          </div>
          {oidcManaged && (
            <div>
              <label>
                <FieldLabel>邮箱</FieldLabel>
                <TextField value={oidcEmail} readOnly placeholder="身份提供商未返回邮箱" />
              </label>
              <span className="mt-1 block text-xs text-gray-400 dark:text-gray-500">请在 OIDC 身份提供商中修改邮箱。</span>
            </div>
          )}
          <label>
            <FieldLabel>显示名称</FieldLabel>
            <TextField value={profileDraft.display_name} onChange={(e) => setProfileDraft((d) => ({ ...d, display_name: e.target.value }))} placeholder="例如：管理员" maxLength={64} />
          </label>
          <label>
            <FieldLabel>{oidcManaged ? '回退头像文字' : '头像文字'}</FieldLabel>
            <TextField value={profileDraft.avatar_text} onChange={(e) => setProfileDraft((d) => ({ ...d, avatar_text: e.target.value }))} placeholder="最多 2 个字符" maxLength={2} />
          </label>
          <label>
            <FieldLabel>{oidcManaged ? '回退头像颜色' : '头像颜色'}</FieldLabel>
            <select
              value={profileDraft.avatar_color}
              onChange={(e) => setProfileDraft((d) => ({ ...d, avatar_color: e.target.value }))}
              className="w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700 focus:border-indigo-500 focus:ring-2 focus:ring-indigo-500/30 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-200"
            >
              {colors.map((color) => <option key={color} value={color}>{color}</option>)}
            </select>
          </label>
        </div>
        <div className="mt-5 flex items-center justify-between gap-3 flex-wrap">
          {error && <p className="text-sm text-indigo-600 dark:text-indigo-400">{error}</p>}
          <button type="submit" disabled={profileSaving} className="ml-auto rounded-lg bg-indigo-600 px-4 py-2 text-sm text-white hover:bg-indigo-700 disabled:opacity-50">
            {profileSaving ? '保存中…' : '保存资料'}
          </button>
        </div>
      </form>

      {profile.local_password === false ? (
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">登录方式</h2>
          <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">当前账号通过 OIDC 单点登录，未设置本地密码，无需修改密码。</p>
        </div>
      ) : (
        <form onSubmit={changePassword} className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">修改密码</h2>
          <p className="mt-1 mb-4 text-sm text-gray-500 dark:text-gray-400">修改成功后当前会话会退出，请使用新密码重新登录。</p>
          <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
            <label><FieldLabel>旧密码</FieldLabel><TextField type="password" value={passwordDraft.old_password} onChange={(e) => setPasswordDraft((d) => ({ ...d, old_password: e.target.value }))} /></label>
            <label><FieldLabel>新密码</FieldLabel><TextField type="password" value={passwordDraft.new_password} onChange={(e) => setPasswordDraft((d) => ({ ...d, new_password: e.target.value }))} placeholder="至少 8 个字符" /></label>
            <label><FieldLabel>确认新密码</FieldLabel><TextField type="password" value={passwordDraft.confirm_password} onChange={(e) => setPasswordDraft((d) => ({ ...d, confirm_password: e.target.value }))} /></label>
          </div>
          <div className="mt-5 flex justify-end">
            <button type="submit" disabled={passwordSaving} className="rounded-lg bg-indigo-600 px-4 py-2 text-sm text-white hover:bg-indigo-700 disabled:opacity-50">
              {passwordSaving ? '修改中…' : '修改密码'}
            </button>
          </div>
        </form>
      )}
    </div>
  )
}

function RetentionSettings({ loading, error, info }) {
  if (loading) return <LoadingSkeleton rows={3} />
  if (!info) return <ErrorState error={error || '系统策略加载失败'} />
  return (
    <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
      <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-2">数据保留策略</h2>
      <p className="text-sm text-gray-500 dark:text-gray-400 mb-5">当前页面展示 Hub 的实际运行策略。自动清理尚未启用，历史数据不会因页面操作被删除。</p>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <StatusItem label="内容保存模式" value={info.content_mode_label || info.content_mode} />
        <StatusItem label="自动清理" value={info.retention?.label || '未启用'} />
        <StatusItem label="展示时区" value={info.timezone || '—'} />
      </div>
      {info.auth_mode && info.auth_mode !== 'password' && (
        <div className="mt-3">
          <StatusItem
            label="登录方式"
            value={
              { oidc: 'OIDC（密码登录已禁用）', 'oidc+password': 'OIDC + 密码' }[info.auth_mode] ||
              info.auth_mode
            }
          />
        </div>
      )}
      <p className="mt-5 text-xs text-gray-400 dark:text-gray-500">详细保留、备份与恢复说明见项目运维文档 <code className="rounded bg-gray-100 px-1.5 py-0.5 dark:bg-gray-700">docs/operations.md</code>。</p>
    </div>
  )
}

function StatusItem({ label, value }) {
  return (
    <div className="rounded-xl bg-gray-50 p-4 dark:bg-gray-700/30">
      <div className="text-xs text-gray-400 dark:text-gray-500">{label}</div>
      <div className="mt-1 text-sm font-semibold text-gray-700 dark:text-gray-200">{value}</div>
    </div>
  )
}
