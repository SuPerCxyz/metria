// 每日使用量堆叠柱：Token 分层；费用与「调用耗时累计」使用单层聚合。

import React, { useMemo } from 'react'
import TrendChart, { COLORS, TOKEN_COLORS } from './TrendChart'
import Segmented from '../ui/Segmented'
import { fmtDuration, fmtTokensShort, fmtUsd, outputTokens } from '../../services/format'

const METRICS = [
  { key: 'tokens', label: 'Token' },
  { key: 'cost', label: '费用' },
  { key: 'duration', label: '时长' },
]

// 颜色统一来自 web/chart-theme.json（见 docs/design-system.md）
const TOKEN = TOKEN_COLORS
const METRIC = COLORS.metric

export default function DailyUsageChart({ series, range, metric, onMetricChange, loading = false, error = null }) {
  // 缓存写入为 0 时不占图层与说明文案（非 0 时自动恢复）
  const hasCacheWrite = useMemo(
    () => (series || []).some((point) => (point.cache_write_tokens ?? 0) > 0),
    [series],
  )
  const tokenCaption = `Token 分层即总 Token 口径：输入 + 输出（含推理）+ 缓存读取${hasCacheWrite ? ' + 缓存写入' : ''}`
  const data = useMemo(() => {
    const points = (series || []).slice().sort((a, b) => String(a.bucket).localeCompare(String(b.bucket)))
    const labels = points.map((point) => point.bucket)
    if (metric === 'cost') {
      return { labels, datasets: [{ label: '费用', color: METRIC.cost, values: points.map((point) => point.cost_micro_usd ?? null) }], formatY: fmtUsd }
    }
    if (metric === 'duration') {
      return { labels, datasets: [{ label: '调用耗时累计', color: METRIC.duration, values: points.map((point) => point.duration_ms ?? null) }], formatY: fmtDuration, unavailable: !points.some((point) => point.duration_ms != null) }
    }
    return {
      labels,
      datasets: [
        { label: '输入', color: TOKEN.input, values: points.map((point) => point.input_tokens ?? null) },
        { label: '输出（含推理）', color: TOKEN.output, values: points.map((point) => outputTokens(point)) },
        { label: '缓存读取', color: TOKEN.cacheRead, values: points.map((point) => point.cache_read_tokens ?? null) },
        ...(hasCacheWrite
          ? [{ label: '缓存写入', color: TOKEN.cacheWrite, values: points.map((point) => point.cache_write_tokens ?? null) }]
          : []),
      ],
      formatY: fmtTokensShort,
    }
  }, [series, metric, hasCacheWrite])

  return (
    <>
      <div className="mb-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">每日使用趋势</h2>
          <Segmented items={METRICS} value={metric} onChange={onMetricChange} />
        </div>
        <p className="mt-1 text-xs text-gray-400 dark:text-gray-500">{tokenCaption}</p>
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
