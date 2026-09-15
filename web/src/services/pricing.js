export const PRICE_FIELDS = [
  ['input_price', '输入 / 百万 Token（美元）'],
  ['output_price', '输出 / 百万 Token（美元）'],
  ['cache_read_price', 'Cache Read / 百万 Token（美元）'],
  ['cache_write_price', 'Cache Write / 百万 Token（美元）'],
  ['reasoning_price', 'Reasoning / 百万 Token（美元）'],
  ['request_price', '每次请求固定费用（美元）'],
]

export const EMPTY_RULE_DRAFT = {
  model_pattern: '',
  provider_pattern: '',
  input_price: '',
  output_price: '',
  cache_read_price: '',
  cache_write_price: '',
  reasoning_price: '',
  request_price: '',
  price_equivalent_to: '',
  price_equivalent_missing_as_free: false,
}

export const EMPTY_LINK_DRAFT = {
  model_pattern: '',
  provider_pattern: '',
  price_equivalent_to: '',
  price_equivalent_missing_as_free: false,
}

export function canonicalModelPattern(value) {
  const text = value.trim().toLowerCase()
  if (!text || /[*?]/.test(text)) return text
  const suffix = text.includes('/') ? text.slice(text.lastIndexOf('/') + 1) : text
  return suffix.split('-').filter((part) => part !== 'free').join('-')
}

function usdToMicro(value) {
  const text = value.trim()
  if (!/^\d+(\.\d{1,6})?$/.test(text)) {
    throw new Error('价格必须是非负美元金额，最多 6 位小数')
  }
  const [whole, fraction = ''] = text.split('.')
  const micro = BigInt(whole) * 1000000n + BigInt(fraction.padEnd(6, '0'))
  return micro.toString()
}

export function serializeRuleDraft(draft, options = {}) {
  const model = canonicalModelPattern(draft.model_pattern)
  const equivalent = (draft.price_equivalent_to || '').trim().toLowerCase()
  const filledPrices = PRICE_FIELDS.filter(([key]) => (draft[key] || '').trim() !== '')
  if (!model) throw new Error('请填写模型名称或匹配模式')
  if (equivalent && filledPrices.length > 0) throw new Error('等价规则不需要重复填写价格')
  if (!equivalent && filledPrices.length === 0) throw new Error('至少填写一项价格；免费模型请填写 0')
  const payload = {
    model_pattern: model,
    provider_pattern: draft.provider_pattern.trim() || '*',
  }
  if (equivalent) {
    payload.price_equivalent_to = equivalent
    if (draft.price_equivalent_missing_as_free) payload.price_equivalent_missing_as_free = true
  }
  if (options.effectiveFrom) payload.effective_from = options.effectiveFrom
  for (const [key] of PRICE_FIELDS) {
    const value = draft[key] || ''
    if (value.trim() !== '') payload[key] = usdToMicro(value)
  }
  return payload
}

// 规则来源分类：价格关联（用户关联等价模型）与用户价格需与目录规则区分展示。
export const RULE_SOURCE_FILTERS = [
  { key: 'all', label: '全部' },
  { key: 'link', label: '价格关联' },
  { key: 'user', label: '用户价格' },
  { key: 'catalog', label: '目录' },
]

const RULE_KIND_META = {
  link: { label: '价格关联', className: 'bg-violet-100 text-violet-700 dark:bg-violet-400/10 dark:text-violet-300' },
  user: { label: '用户价格', className: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-400/10 dark:text-emerald-300' },
  openrouter: { label: 'OpenRouter', className: 'bg-gray-100 text-gray-600 dark:bg-gray-700/40 dark:text-gray-300' },
  litellm: { label: 'LiteLLM', className: 'bg-gray-100 text-gray-600 dark:bg-gray-700/40 dark:text-gray-300' },
  custom: { label: '自定义目录', className: 'bg-gray-100 text-gray-600 dark:bg-gray-700/40 dark:text-gray-300' },
  other: { label: '其他', className: 'bg-gray-100 text-gray-600 dark:bg-gray-700/40 dark:text-gray-300' },
}

const CATALOG_KINDS = ['openrouter', 'litellm', 'custom']

export function pricingRuleKind(rule) {
  switch (rule?.source) {
    case 'openrouter_catalog': return 'openrouter'
    case 'litellm_catalog': return 'litellm'
    case 'custom_http_catalog': return 'custom'
    case 'user_override': return rule.price_equivalent_to ? 'link' : 'user'
    default: return 'other'
  }
}

export function pricingRuleKindMeta(rule) {
  return RULE_KIND_META[pricingRuleKind(rule)] || RULE_KIND_META.other
}

export function filterPricingRules(rules, kind) {
  if (!kind || kind === 'all') return rules
  return rules.filter((rule) => {
    const ruleKind = pricingRuleKind(rule)
    return kind === 'catalog' ? CATALOG_KINDS.includes(ruleKind) : ruleKind === kind
  })
}
