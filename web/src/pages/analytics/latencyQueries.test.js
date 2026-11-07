import '../../../test-support/setup-dom.js'

import assert from 'node:assert/strict'
import test from 'node:test'
import React from 'react'
import { render, waitFor } from '@testing-library/react'

import { useQuery } from '../../hooks/useQuery.js'
import {
  latencySeriesEnabled,
  latencySummaryEnabled,
} from './latencyQueries.js'

function Harness({ tab, fetcher }) {
  useQuery('latency-gating-test', fetcher, { enabled: latencySummaryEnabled(tab) })
  return null
}

test('latency summary is requested only on latency or detail tabs', async () => {
  let calls = 0
  const fetcher = async () => {
    calls += 1
    return { count: 0 }
  }

  const view = render(React.createElement(Harness, { tab: 'tokens', fetcher }))
  await new Promise((resolve) => setTimeout(resolve, 20))
  assert.equal(calls, 0, '非延迟/非详情标签不得请求 /usage/latency')

  view.rerender(React.createElement(Harness, { tab: 'latency', fetcher }))
  await waitFor(() => assert.equal(calls, 1))
  assert.equal(latencySeriesEnabled('latency'), true)

  view.rerender(React.createElement(Harness, { tab: 'agent-detail', fetcher }))
  await new Promise((resolve) => setTimeout(resolve, 20))
  assert.equal(calls, 1, '详情标签复用摘要查询，不重复请求')
  assert.equal(latencySeriesEnabled('agent-detail'), false)
  assert.equal(latencySummaryEnabled('model-detail'), true)
})
