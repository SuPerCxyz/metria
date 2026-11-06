// 刷新窗口语义：用于顶栏区分「数据已刷新」与「刷新失败」。

import assert from 'node:assert/strict'
import test from 'node:test'
import {
  QUERY_CACHE_TTL_MS,
  beginRefreshWindow,
  clearQueryCache,
  endRefreshWindow,
  getQueryCache,
  isQueryCacheFresh,
  markRefreshFailure,
  putQueryCache,
  sharedRequest,
} from './useQuery.js'
import { api } from '../services/api.js'

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


test('SWR cache returns data and expires after TTL', () => {
  clearQueryCache()
  putQueryCache('cache-key', { value: 1 }, 1_000)
  const entry = getQueryCache('cache-key')
  assert.deepEqual(entry.data, { value: 1 })
  assert.equal(isQueryCacheFresh(entry, 1_000 + QUERY_CACHE_TTL_MS - 1), true)
  assert.equal(isQueryCacheFresh(entry, 1_000 + QUERY_CACHE_TTL_MS), false)
  clearQueryCache()
  assert.equal(getQueryCache('cache-key'), null)
})

test('一个调用者 abort 只取消自己的等待，不影响其他复用者', async () => {
  const key = 'signal-shared-one'
  let release
  const gate = new Promise((resolve) => { release = resolve })
  let underlyingSignal = null
  const fetcher = ({ signal }) => {
    underlyingSignal = signal
    return gate.then(() => 'ok')
  }
  const first = new AbortController()
  const second = new AbortController()
  const p1 = sharedRequest(key, fetcher, { signal: first.signal })
  const p2 = sharedRequest(key, fetcher, { signal: second.signal })
  const rejected = assert.rejects(p1, (error) => error.name === 'AbortError')
  first.abort()
  await rejected
  assert.equal(underlyingSignal.aborted, false, '第一个调用者 abort 不得取消共享请求')
  release()
  assert.equal(await p2, 'ok')
})

test('全部调用者 abort 后底层请求被取消', async () => {
  const key = 'signal-shared-all'
  let underlyingSignal = null
  const fetcher = ({ signal }) => {
    underlyingSignal = signal
    return new Promise((_resolve, reject) => {
      signal.addEventListener('abort', () => {
        const error = new Error('Aborted')
        error.name = 'AbortError'
        reject(error)
      }, { once: true })
    })
  }
  const first = new AbortController()
  const second = new AbortController()
  const p1 = sharedRequest(key, fetcher, { signal: first.signal })
  const p2 = sharedRequest(key, fetcher, { signal: second.signal })
  const both = Promise.allSettled([p1, p2])
  first.abort()
  second.abort()
  const results = await both
  assert.deepEqual(results.map((result) => result.status), ['rejected', 'rejected'])
  assert.equal(underlyingSignal.aborted, true, '全部等待者取消后应 abort 底层请求')
})


test('api forwards an AbortSignal to fetch', async () => {
  const original = globalThis.fetch
  let seen = null
  globalThis.fetch = async (_url, opts) => {
    seen = opts.signal
    return { ok: true, status: 200, json: async () => ({ ok: true }) }
  }
  try {
    const controller = new AbortController()
    await api('/signal-test', { signal: controller.signal })
    assert.equal(seen, controller.signal)
  } finally {
    globalThis.fetch = original
  }
})
