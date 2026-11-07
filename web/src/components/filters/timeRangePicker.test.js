import '../../../test-support/setup-dom.js'

import assert from 'node:assert/strict'
import test from 'node:test'
import React from 'react'
import { fireEvent, render, waitFor } from '@testing-library/react'

import TimeRangePicker from './TimeRangePicker.jsx'
import { TimeRangeProvider, useTimeRange } from '../../hooks/useTimeRange.jsx'

function RangeProbe() {
  const { range } = useTimeRange()
  return React.createElement(
    'span',
    { 'data-testid': 'range' },
    `${range.from}|${range.to}`,
  )
}

test('applies a quick preset and keeps the trigger usable', async () => {
  const view = render(
    React.createElement(
      TimeRangeProvider,
      null,
      React.createElement(TimeRangePicker),
      React.createElement(RangeProbe),
    ),
  )
  const before = view.getByTestId('range').textContent

  fireEvent.click(view.getByRole('button', { name: /时间范围/ }))
  const preset = await view.findByRole(
    'button',
    { name: '最近 1 小时' },
    { timeout: 5000 },
  )
  fireEvent.click(preset)

  await waitFor(() => {
    assert.notEqual(view.getByTestId('range').textContent, before)
  })
  const [from, to] = view
    .getByTestId('range')
    .textContent.split('|')
    .map((value) => Date.parse(value))
  assert.ok(
    Math.abs((to - from) - 3_600_000) < 5_000,
    '快捷范围应切换为最近 1 小时',
  )
  assert.ok(view.getByRole('button', { name: /时间范围/ }), '触发器必须保持可用')
})
