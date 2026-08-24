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

export function serializeRuleDraft(draft) {
  const model = canonicalModelPattern(draft.model_pattern)
  const filledPrices = PRICE_FIELDS.filter(([key]) => draft[key].trim() !== '')
  if (!model) throw new Error('请填写模型名称或匹配模式')
  if (filledPrices.length === 0) throw new Error('至少填写一项价格；免费模型请填写 0')
  const payload = {
    model_pattern: model,
    provider_pattern: draft.provider_pattern.trim() || '*',
  }
  for (const [key] of PRICE_FIELDS) {
    if (draft[key].trim() !== '') payload[key] = usdToMicro(draft[key])
  }
  return payload
}
