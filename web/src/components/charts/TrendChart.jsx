// 主趋势图（Chart.js 折线/面积图）：支持多条线（datasets），数据降采样保证 ≥30 点。
// legendDisplay=false 时隐藏内置图例（由调用方提供自定义图例，如全部模型 chips）。

import React, { useEffect, useRef } from 'react'
import Chart from 'chart.js/auto'
import { downsampleIndices, formatTimeLabel, isRangeLongerThanDay } from './trendChartLabels'
import chartTheme from '../../../chart-theme.json'

export const PALETTE = chartTheme.palette

export default function TrendChart({ labels, values, datasets, tooltipLabels, range, height = 320, color = '#6366f1', formatY, prefix = '', ariaLabel = '趋势图', onLegendClick, legendDisplay = true, chartType = 'line', stacked = false }) {
  const ref = useRef(null)
  const chartRef = useRef(null)
  const showDateOnAxis = isRangeLongerThanDay(range)

  useEffect(() => {
    if (!ref.current) return
    const ctx = ref.current.getContext('2d')
    if (chartRef.current) chartRef.current.destroy()

    const ds = datasets
      ? datasets.map((d, i) => ({ label: d.label || '', values: d.values || [], color: d.color || PALETTE[i % PALETTE.length], fill: d.fill ?? true, hidden: d.hidden ?? false }))
      : [{ label: '', values: values || [], color, fill: true }]

    // 按下采样后的索引对齐 labels 与所有数据集
    const idx = downsampleIndices((labels || []).length)
    const ttip = tooltipLabels ? idx.map((i) => tooltipLabels[i]) : idx.map((i) => formatTimeLabel(labels[i], true))
    const data = {
      labels: idx.map((i) => formatTimeLabel(labels[i], false, showDateOnAxis)),
      datasets: ds.map((d) => ({
        label: d.label,
        data: idx.map((i) => d.values[i] == null ? null : d.values[i]),
        borderColor: d.color,
        backgroundColor: chartType === 'bar' ? `${d.color}cc` : `${d.color}${chartTheme.fillAlphaHex}`,
        fill: d.fill,
        hidden: d.hidden,
        tension: chartType === 'line' ? chartTheme.tension : undefined,
        borderWidth: chartType === 'bar' ? 1 : chartTheme.lineWidth,
        borderRadius: chartType === 'bar' ? 3 : undefined,
        categoryPercentage: chartType === 'bar' ? 0.55 : undefined,
        barPercentage: chartType === 'bar' ? 0.85 : undefined,
        maxBarThickness: chartType === 'bar' ? 36 : undefined,
        pointRadius: chartType === 'line' ? 0 : undefined,
        pointHoverRadius: chartType === 'line' ? 4 : undefined,
        pointBackgroundColor: d.color,
      })),
    }

    const chart = new Chart(ctx, {
      type: chartType,
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
            stacked: chartType === 'bar' && stacked,
            ticks: { maxTicksLimit: chartTheme.xMaxTicks, maxRotation: 0, color: chartTheme.axisColor, font: { size: chartTheme.fontSize } },
          },
          y: {
            grid: { color: chartTheme.gridColor },
            stacked: chartType === 'bar' && stacked,
            beginAtZero: chartType === 'bar',
            ticks: { color: chartTheme.axisColor, font: { size: chartTheme.fontSize }, callback: (v) => (formatY ? formatY(v) : v.toLocaleString()) },
          },
        },
      },
    })
    chartRef.current = chart
    return () => { if (chartRef.current) chartRef.current.destroy() }
  }, [labels, datasets, values, tooltipLabels, range, showDateOnAxis, color, height, onLegendClick, legendDisplay, chartType, stacked])

  return <div style={{ height }}><canvas ref={ref} role="img" aria-label={`${ariaLabel}，共 ${labels?.length || 0} 个数据点`} /></div>
}
