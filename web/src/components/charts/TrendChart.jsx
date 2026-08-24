// 主趋势图（Chart.js 折线/面积图）：支持多条线（datasets），数据降采样保证 ≥30 点。

import React, { useEffect, useRef } from 'react'
import Chart from 'chart.js/auto'
import { formatTimeLabel } from './trendChartLabels'

// 数据点过多时降采样：保留 maxPoints 个点（≥30）
function downsample(data, maxPoints = 240) {
  if (data.length <= maxPoints) return data
  const step = Math.ceil(data.length / maxPoints)
  const out = []
  for (let i = 0; i < data.length; i += step) out.push(data[i])
  return out
}

const PALETTE = ['#6366f1', '#10b981', '#f59e0b', '#ef4444', '#06b6d4', '#8b5cf6', '#ec4899', '#84cc16']

export default function TrendChart({ labels, values, datasets, tooltipLabels, height = 320, color = '#6366f1', formatY, prefix = '', ariaLabel = '趋势图', onLegendClick }) {
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
        backgroundColor: `${d.color}18`,
        fill: d.fill,
        hidden: d.hidden,
        tension: 0.3,
        borderWidth: 2,
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
            display: ds.length > 1 || ds.some((d) => d.label),
            labels: { color: '#9ca3af', boxWidth: 12, font: { size: 11 }, usePointStyle: true, padding: 12 },
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
            ticks: { maxTicksLimit: 10, maxRotation: 0, color: '#9ca3af', font: { size: 11 } },
          },
          y: {
            grid: { color: 'rgba(156,163,175,0.12)' },
            ticks: { color: '#9ca3af', font: { size: 11 }, callback: (v) => (formatY ? formatY(v) : v.toLocaleString()) },
          },
        },
      },
    })
    chartRef.current = chart
    return () => { if (chartRef.current) chartRef.current.destroy() }
  }, [labels, datasets, values, tooltipLabels, color, height, onLegendClick])

  return <div style={{ height }}><canvas ref={ref} role="img" aria-label={`${ariaLabel}，共 ${labels?.length || 0} 个数据点`} /></div>
}
