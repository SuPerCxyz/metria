// 刷新窗口语义：用于顶栏区分「数据已刷新」与「刷新失败」。

import assert from 'node:assert/strict'
import test from 'node:test'
import { beginRefreshWindow, endRefreshWindow, markRefreshFailure } from './useQuery.js'

test('refresh window reports success when no request failed', () => {
  beginRefreshWindow()
  assert.equal(endRefreshWindow(), false)
})

test('refresh window reports failure when a request failed', () => {
  beginRefreshWindow()
  markRefreshFailure()
  assert.equal(endRefreshWindow(), true)
  // 结束后状态复位，不影响下一次
  assert.equal(endRefreshWindow(), false)
})

test('failures outside a refresh window are ignored', () => {
  markRefreshFailure()
  beginRefreshWindow()
  assert.equal(endRefreshWindow(), false)
})

test('repeated failures within one window collapse to a single failure flag', () => {
  beginRefreshWindow()
  markRefreshFailure()
  markRefreshFailure()
  assert.equal(endRefreshWindow(), true)
  assert.equal(endRefreshWindow(), false)
})
