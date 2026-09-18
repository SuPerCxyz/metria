// 页面数据可用性：区分缺失值与真实的 0，避免空卡片和假数据。

const TOKEN_FIELDS = ['input_tokens', 'output_tokens', 'reasoning_tokens', 'cache_read_tokens', 'cache_write_tokens']

export function isAvailable(value) {
  return value !== null && value !== undefined && !(typeof value === 'string' && value.trim() === '')
}

export function hasAnyToken(value) {
  return TOKEN_FIELDS.some((field) => isAvailable(value?.[field]))
}

export function averagePerCall(total, calls) {
  return isAvailable(total) && Number(calls) > 0 ? Number(total) / Number(calls) : null
}

export function getVisibleColumns(columns, rows) {
  const items = rows || []
  if (items.length === 0) return columns
  return columns.filter((column) => {
    if (!column.hideWhenEmpty) return true
    return items.some((row) => isAvailable(column.availabilityValue
      ? column.availabilityValue(row)
      : column.sortValue
        ? column.sortValue(row)
        : row?.[column.key]))
  })
}
