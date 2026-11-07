import '../../test-support/setup-dom.js'

import assert from 'node:assert/strict'
import test from 'node:test'
import React from 'react'
import { act, renderHook, waitFor } from '@testing-library/react'

import { usePagedSessionList } from './usePagedSessionList.js'
import { beginRefreshWindow, endRefreshWindow, useQueryActivity } from './useQuery.js'

test('refresh event resets cursor, reloads first page, and appends without duplicates', async () => {
  const calls = []
  const fetcher = async (url) => {
    calls.push(url)
    if (url.includes('cursor=')) {
      return {
        items: [{ id: 'new-1' }, { id: 'old-2' }],
        total: 2,
        next_cursor: null,
        has_more: false,
      }
    }
    return {
      items: [{ id: 'new-1' }],
      total: 2,
      next_cursor: 'cursor-1',
      has_more: true,
    }
  }

  const { result } = renderHook(() =>
    usePagedSessionList('/sessions/x/calls', 'items', 1, fetcher),
  )
  await waitFor(() => assert.equal(result.current.items.length, 1))
  assert.equal(calls.length, 1)

  act(() => result.current.loadMore())
  await waitFor(() => assert.equal(result.current.items.length, 2))
  assert.ok(calls[1].includes('cursor=cursor-1'))
  assert.deepEqual(
    result.current.items.map((item) => item.id),
    ['new-1', 'old-2'],
    '分页追加必须按 id 去重',
  )

  act(() => {
    window.dispatchEvent(new window.CustomEvent('metria:refresh'))
  })
  await waitFor(() => assert.equal(result.current.items.length, 1))
  assert.ok(!calls.at(-1).includes('cursor='), '刷新必须重置游标并重新拉第一页')
  assert.deepEqual(result.current.items.map((item) => item.id), ['new-1'])
})

test('failed paged request marks the header refresh window as failed', async () => {
  const fetcher = async () => {
    throw new Error('paged request failed')
  }
  beginRefreshWindow()
  try {
    const { result } = renderHook(() =>
      usePagedSessionList('/sessions/x/calls', 'items', 1, fetcher),
    )
    await waitFor(() => assert.ok(result.current.error))
    assert.equal(endRefreshWindow(), true, '分页失败必须让顶栏提示刷新失败')
  } finally {
    endRefreshWindow()
  }
})

test('unmount aborts the in-flight paged request without marking refresh failure', async () => {
  let signal
  const fetcher = (_url, options) => {
    signal = options?.signal
    return new Promise((_resolve, reject) => {
      signal.addEventListener('abort', () => {
        const error = new Error('Aborted')
        error.name = 'AbortError'
        reject(error)
      })
    })
  }

  beginRefreshWindow()
  let windowClosed = false
  try {
    const { unmount } = renderHook(() =>
      usePagedSessionList('/sessions/x/calls', 'items', 1, fetcher),
    )
    await waitFor(() => assert.ok(signal))
    unmount()
    assert.equal(signal.aborted, true, '卸载必须取消本组件的在飞请求')
    await new Promise((resolve) => setTimeout(resolve, 10))
    const failed = endRefreshWindow()
    windowClosed = true
    assert.equal(
      failed,
      false,
      '取消在飞请求不等于刷新失败，不应污染顶栏状态',
    )
  } finally {
    if (!windowClosed) endRefreshWindow()
  }
})

test('paged request participates in the global query activity count', async () => {
  let release
  const fetcher = () => new Promise((resolve) => { release = resolve })

  const { result } = renderHook(() => ({
    list: usePagedSessionList('/sessions/x/calls', 'items', 1, fetcher),
    activity: useQueryActivity(),
  }))
  await waitFor(() => assert.ok(result.current.activity >= 1))

  await act(async () => {
    release({ items: [], total: 0, next_cursor: null, has_more: false })
  })
  await waitFor(() => assert.equal(result.current.activity, 0))
})
