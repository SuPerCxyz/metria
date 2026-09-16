// 每日使用量堆叠柱：Token 分层；费用与活跃时长使用单层聚合。

import React, { useMemo } from 'react'
import TrendChart from './TrendChart'
import Segmented from '../ui/Segmented'
import { fmtDuration, fmtTokensShort, fmtUsd, outputTokens } from '../../services/format'

const METRICS = [
  { key: 'tokens', label: 'Token' },
  { key: 'cost', label: '费用' },
  { key: 'duration', label: '时长' },
]

const COLORS = {
  input: '#6366f1',
  output: '#10b981',
  cache: '#94a3b8',
  cacheWrite: '#cbd5e1',
  cost: '#f59e0b',
  duration: '#06b6d4',
}

export default function DailyUsageChart({ series, range, metric, onMetricChange, loading = false, error = null }) {
  const data = useMemo(() => {
    const points = (series || []).slice().sort((a, b) => String(a.bucket).localeCompare(String(b.bucket)))
    const labels = points.map((point) => point.bucket)
    if (metric === 'cost') {
      return { labels, datasets: [{ label: '费用', color: COLORS.cost, values: points.map((point) => point.cost_micro_usd ?? null) }], formatY: fmtUsd }
    }
    if (metric === 'duration') {
      return { labels, datasets: [{ label: '活跃时长', color: COLORS.duration, values: points.map((point) => point.duration_ms ?? null) }], formatY: fmtDuration, unavailable: !points.some((point) => point.duration_ms != null) }
    }
    return {
      labels,
      datasets: [
        { label: '输入', color: COLORS.input, values: points.map((point) => point.input_tokens ?? null) },
        { label: '输出（含推理）', color: COLORS.output, values: points.map((point) => outputTokens(point)) },
        { label: '缓存读取', color: COLORS.cache, values: points.map((point) => point.cache_read_tokens ?? null) },
        { label: '缓存写入', color: COLORS.cacheWrite, values: points.map((point) => point.cache_write_tokens ?? null) },
      ],
      formatY: fmtTokensShort,
    }
  }, [series, metric])

  return (
    <>
      <div className="mb-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">每日使用趋势</h2>
          <Segmented items={METRICS} value={metric} onChange={onMetricChange} />
        </div>
        <p className="mt-1 text-xs text-gray-400 dark:text-gray-500">Token 分层即总 Token 口径：输入 + 输出（含推理）+ 缓存读取 + 缓存写入</p>
      </div>
      {loading && !series ? <div className="py-12 text-center text-sm text-gray-400 dark:text-gray-500">加载中…</div> : error ? <div className="py-12 text-center text-sm text-amber-600 dark:text-amber-400">每日趋势加载失败，请刷新重试。</div> : data.labels.length === 0 ? <div className="py-12 text-center text-sm text-gray-400 dark:text-gray-500">当前范围无数据</div> : data.unavailable ? <div className="py-12 text-center text-sm text-gray-400 dark:text-gray-500">当前范围没有可观测的调用时长</div> : (
        <TrendChart
          labels={data.labels}
          datasets={data.datasets}
          range={range}
          height={280}
          formatY={data.formatY}
          chartType="bar"
          stacked={metric === 'tokens'}
          ariaLabel="每日使用趋势堆叠柱图"
        />
      )}
    </>
  )
}
