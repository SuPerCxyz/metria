export function isUnavailable(value) {
  return value == null || value === '' || (typeof value === 'number' && Number.isNaN(value))
}

export function compareSortValues(left, right, direction = 1) {
  const leftMissing = isUnavailable(left)
  const rightMissing = isUnavailable(right)
  if (leftMissing || rightMissing) {
    if (leftMissing && rightMissing) return 0
    return leftMissing ? 1 : -1
  }

  let result
  if (typeof left === 'number' && typeof right === 'number') {
    result = left - right
  } else {
    result = String(left).localeCompare(String(right), undefined, { numeric: true, sensitivity: 'base' })
  }
  return result * direction
}

export function sortItems(items, getValue, direction = 1) {
  return (items || [])
    .map((item, index) => ({ item, index }))
    .sort((left, right) => compareSortValues(getValue(left.item), getValue(right.item), direction) || left.index - right.index)
    .map(({ item }) => item)
}
