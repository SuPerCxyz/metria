// 主趋势图（Chart.js 折线/面积图）：单主指标，支持数据降采样。

import React, { useEffect, useRef } from 'react'
import Chart from 'chart.js/auto'

// 数据点过多时降采样：保留 maxPoints 个点
function downsample(data, maxPoints = 120) {
  if (data.length <= maxPoints) return data
  const step = Math.ceil(data.length / maxPoints)
  const out = []
  for (let i = 0; i < data.length; i += step) out.push(data[i])
  return out
}

export default function TrendChart({ labels, values, height = 320, color = '#6366f1', formatY, prefix = '' }) {
  const ref = useRef(null)
  const chartRef = useRef(null)

  useEffect(() => {
    if (!ref.current) return
    const ctx = ref.current.getContext('2d')
    if (chartRef.current) chartRef.current.destroy()

    const pts = downsample(values.map((v, i) => ({ v, l: labels[i] })))
    const chart = new Chart(ctx, {
      type: 'line',
      data: {
        labels: pts.map((p) => p.l),
        datasets: [{
          data: pts.map((p) => p.v),
          borderColor: color,
          backgroundColor: `${color}18`,
          fill: true,
          tension: 0.3,
          borderWidth: 2,
          pointRadius: 0,
          pointHoverRadius: 4,
          pointBackgroundColor: color,
        }],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: { mode: 'index', intersect: false },
        plugins: {
          legend: { display: false },
          tooltip: {
            callbacks: {
              label: (c) => formatY ? formatY(c.parsed.y) : `${prefix}${c.parsed.y.toLocaleString()}`,
            },
          },
        },
        scales: {
          x: {
            grid: { display: false },
            ticks: { maxTicksLimit: 8, color: '#9ca3af', font: { size: 11 } },
          },
          y: {
            grid: { color: 'rgba(156,163,175,0.12)' },
            ticks: {
              color: '#9ca3af',
              font: { size: 11 },
              callback: (v) => formatY ? formatY(v) : v.toLocaleString(),
            },
          },
        },
      },
    })
    chartRef.current = chart
    return () => { if (chartRef.current) chartRef.current.destroy() }
  }, [labels, values, color, height]) // eslint-disable-line react-hooks/exhaustive-deps

  return <div style={{ height }}><canvas ref={ref} /></div>
}
