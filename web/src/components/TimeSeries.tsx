import { useEffect, useMemo, useState } from 'preact/hooks'
import uPlot from 'uplot'
import 'uplot/dist/uPlot.min.css'

/** 轻量 uPlot 时间序列封装。 */
export function TimeSeries({
  data,
  height = 220,
  label,
}: {
  data: { bucket: string; [k: string]: any }[]
  height?: number
  label?: string
}) {
  const ref = useMemo(() => ({ current: null as HTMLDivElement | null }), [])
  const [plot, setPlot] = useState<uPlot | null>(null)

  useEffect(() => {
    if (!ref.current || data.length === 0) return
    if (plot) {
      plot.setData([data.map((d) => new Date(d.bucket).getTime() / 1000), data.map((d) => Number(d.value ?? 0))])
      return
    }
    const opts: uPlot.Options = {
      width: ref.current.clientWidth,
      height,
      legend: { show: false },
      scales: { x: { time: true }, y: { auto: true } },
      axes: [
        { stroke: '#6b7280' },
        {
          stroke: '#6b7280',
          values: (_self, ticks) => ticks.map((t) => (Math.abs(t) >= 1_000_000 ? `${(t / 1_000_000).toFixed(1)}M` : t >= 1000 ? `${(t / 1000).toFixed(1)}k` : String(t))),
        },
      ],
      series: [
        {},
        {
          label: label || '值',
          stroke: '#2563eb',
          width: 1.5,
          points: { size: 3, show: data.length <= 100 },
          fill: 'rgba(37,99,235,0.08)',
        },
      ],
    }
    const p = new uPlot(opts, [data.map((d) => new Date(d.bucket).getTime() / 1000), data.map((d) => Number(d.value ?? 0))], ref.current)
    setPlot(p)
    const ro = new ResizeObserver(() => {
      p.setSize({ width: ref.current?.clientWidth || 300, height })
    })
    ro.observe(ref.current)
    return () => {
      ro.disconnect()
      p.destroy()
    }
  }, [data, height])

  return <div ref={ref} class="timeseries" />
}
