// 主趋势图（Chart.js 折线/面积图）：支持多条线（datasets），数据降采样保证 ≥30 点。
// legendDisplay=false 时隐藏内置图例（由调用方提供自定义图例，如全部模型 chips）。

import React, { useEffect, useRef } from 'react'
import Chart from 'chart.js/auto'
import { formatTimeLabel } from './trendChartLabels'
import chartTheme from '../../../chart-theme.json'

// 数据点过多时降采样：保留 maxPoints 个点（≥30）
function downsample(data, maxPoints = 240) {
  if (data.length <= maxPoints) return data
  const step = Math.ceil(data.length / maxPoints)
  const out = []
  for (let i = 0; i < data.length; i += step) out.push(data[i])
  return out
}

export const PALETTE = chartTheme.palette

export default function TrendChart({ labels, values, datasets, tooltipLabels, height = 320, color = '#6366f1', formatY, prefix = '', ariaLabel = '趋势图', onLegendClick, legendDisplay = true }) {
  const ref = useRef(null)
  const chartRef = useRef(null)

  useEffect(() => {
    if (!ref.current) return
    const ctx = ref.current.getContext('2d')
    if (chartRef.current) chartRef.current.destroy()

    const ds = datasets
      ? datasets.map((d, i) => ({ label: d.label || '', values: d.values || [], color: d.color || PALETTE[i % PALETTE.length], fill: d.fill ?? true, hidden: d.hidden ?? false }))
      : [{ label: '', values: values || [], color, fill: true }]

    // 按下采样后的索引对齐 labels 与所有数据集
    const idx = downsample((labels || []).map((_, i) => i))
    const ttip = tooltipLabels ? idx.map((i) => tooltipLabels[i]) : idx.map((i) => formatTimeLabel(labels[i], true))
    const data = {
      labels: idx.map((i) => formatTimeLabel(labels[i])),
      datasets: ds.map((d) => ({
        label: d.label,
        data: idx.map((i) => d.values[i] ?? 0),
        borderColor: d.color,
        backgroundColor: `${d.color}${chartTheme.fillAlphaHex}`,
        fill: d.fill,
        hidden: d.hidden,
        tension: chartTheme.tension,
        borderWidth: chartTheme.lineWidth,
        pointRadius: 0,
        pointHoverRadius: 4,
        pointBackgroundColor: d.color,
      })),
    }

    const chart = new Chart(ctx, {
      type: 'line',
      data,
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: { mode: 'index', intersect: false },
        plugins: {
          legend: {
            display: legendDisplay && (ds.length > 1 || ds.some((d) => d.label)),
            labels: { color: chartTheme.axisColor, boxWidth: 12, font: { size: chartTheme.fontSize }, usePointStyle: true, padding: 12 },
            ...(onLegendClick ? {
              onClick: (_event, legendItem, legend) => {
                const datasetIndex = legendItem.datasetIndex
                const dataset = ds[datasetIndex]
                legend.chart.setDatasetVisibility(datasetIndex, !legend.chart.isDatasetVisible(datasetIndex))
                legend.chart.update()
                if (dataset?.label) onLegendClick(dataset.label)
              },
            } : {}),
          },
          tooltip: {
            callbacks: {
              title: (items) => {
                const i = items[0]?.dataIndex
                return ttip[i] || items[0]?.label || ''
              },
              label: (c) => `${c.dataset.label ? `${c.dataset.label}：` : ''}${formatY ? formatY(c.parsed.y) : `${prefix}${c.parsed.y.toLocaleString()}`}`,
            },
          },
        },
        scales: {
          x: {
            grid: { display: false },
            ticks: { maxTicksLimit: chartTheme.xMaxTicks, maxRotation: 0, color: chartTheme.axisColor, font: { size: chartTheme.fontSize } },
          },
          y: {
            grid: { color: chartTheme.gridColor },
            ticks: { color: chartTheme.axisColor, font: { size: chartTheme.fontSize }, callback: (v) => (formatY ? formatY(v) : v.toLocaleString()) },
          },
        },
      },
    })
    chartRef.current = chart
    return () => { if (chartRef.current) chartRef.current.destroy() }
  }, [labels, datasets, values, tooltipLabels, color, height, onLegendClick, legendDisplay])

  return <div style={{ height }}><canvas ref={ref} role="img" aria-label={`${ariaLabel}，共 ${labels?.length || 0} 个数据点`} /></div>
}
