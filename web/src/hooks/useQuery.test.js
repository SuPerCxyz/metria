// 刷新窗口语义：用于顶栏区分「数据已刷新」与「刷新失败」。

import assert from 'node:assert/strict'
import test from 'node:test'
import { beginRefreshWindow, endRefreshWindow, markRefreshFailure, sharedRequest } from './useQuery.js'

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

test('相同查询键的在飞请求合并为一次网络请求，双方拿到同一结果', async () => {
  let calls = 0
  const fetcher = () => {
    calls += 1
    return Promise.resolve(`data-${calls}`)
  }
  const key = 'test-dedup-same-key'
  const first = sharedRequest(key, fetcher)
  const second = sharedRequest(key, fetcher)
  assert.equal(calls, 1)
  assert.equal(await first, 'data-1')
  assert.equal(await second, 'data-1')
  // 完成后条目清理，下一次调用重新发请求
  const third = sharedRequest(key, fetcher)
  assert.equal(calls, 2)
  assert.equal(await third, 'data-2')
})

test('合并使用发起时的 fetcher，不受后来的闭包影响', async () => {
  const key = 'test-dedup-fetcher-closure'
  const first = sharedRequest(key, () => Promise.resolve('from-first-fetcher'))
  const late = sharedRequest(key, () => Promise.resolve('from-late-fetcher'))
  assert.equal(await first, 'from-first-fetcher')
  assert.equal(await late, 'from-first-fetcher')
})

test('force 刷新穿透在飞去重，重新发起请求', async () => {
  // 手动刷新的语义是「拿点击之后的最新数据」：若复用点击前在飞的 Promise，
  // 返回的是更早的快照，等于刷新没生效（spec hub-query-performance 场景）。
  const key = 'test-force-refresh'
  let calls = 0
  const first = sharedRequest(key, () => Promise.resolve(`data-${++calls}`))
  const forced = sharedRequest(key, () => Promise.resolve(`data-${++calls}`), { force: true })
  assert.equal(calls, 2, 'force 不得复用在飞请求')
  assert.equal(await first, 'data-1')
  assert.equal(await forced, 'data-2', 'force 必须拿到新一次请求的结果')
})

test('force 刷新结束后，后续普通请求仍正常合并', async () => {
  const key = 'test-force-then-normal'
  let calls = 0
  const fetcher = () => Promise.resolve(`v${++calls}`)
  await sharedRequest(key, fetcher, { force: true })
  const a = sharedRequest(key, fetcher)
  const b = sharedRequest(key, fetcher)
  assert.equal(calls, 2, 'force 之后普通调用恢复合并语义')
  assert.equal(await a, await b)
})

test('不同查询键不合并', async () => {
  let calls = 0
  const fetcher = () => {
    calls += 1
    return Promise.resolve(calls)
  }
  const a = sharedRequest('test-dedup-key-a', fetcher)
  const b = sharedRequest('test-dedup-key-b', fetcher)
  assert.equal(calls, 2)
  await Promise.all([a, b])
})

test('失败的在飞请求结束后清理，后续调用可重试', async () => {
  const key = 'test-dedup-failure'
  await assert.rejects(sharedRequest(key, () => Promise.reject(new Error('boom'))))
  const retry = sharedRequest(key, () => Promise.resolve('recovered'))
  assert.equal(await retry, 'recovered')
})
