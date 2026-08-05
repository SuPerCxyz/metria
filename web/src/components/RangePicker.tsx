import { useState } from 'preact/hooks'
import { quickRange, setRange, useRangeStore } from '../hooks/useQuery'

const PRESETS = ['今天', '昨天', '最近 24 小时', '最近 7 天', '最近 30 天']

export function RangePicker() {
  const current = useRangeStore()
  const [from, setFrom] = useState(current.from.slice(0, 16))
  const [to, setTo] = useState(current.to.slice(0, 16))
  const [tz, setTz] = useState(current.timezone)
  const [granularity, setGranularity] = useState<'hour' | 'day'>(current.granularity)
  const [open, setOpen] = useState(false)

  const apply = () => {
    setRange({
      from: new Date(from).toISOString(),
      to: new Date(to).toISOString(),
      timezone: tz || 'UTC',
      granularity,
    })
    setOpen(false)
  }

  return (
    <div class="range-picker">
      <button type="button" class="btn" onClick={() => setOpen(!open)}>
        时间范围：{new Date(current.from).toLocaleDateString()} ~ {new Date(current.to).toLocaleDateString()}
      </button>
      {open && (
        <div class="range-panel">
          <div class="range-presets">
            {PRESETS.map((p) => (
              <button
                type="button"
                key={p}
                class="btn small"
                onClick={() => {
                  const r = quickRange(p)
                  setRange(r)
                  setFrom(r.from.slice(0, 16))
                  setTo(r.to.slice(0, 16))
                  setOpen(false)
                }}
              >
                {p}
              </button>
            ))}
          </div>
          <div class="range-row">
            <label>
              开始
              <input type="datetime-local" value={from} onChange={(e) => setFrom((e.target as HTMLInputElement).value)} />
            </label>
            <label>
              结束
              <input type="datetime-local" value={to} onChange={(e) => setTo((e.target as HTMLInputElement).value)} />
            </label>
            <label>
              时区
              <input type="text" value={tz} onChange={(e) => setTz((e.target as HTMLInputElement).value)} placeholder="Asia/Shanghai" />
            </label>
            <label>
              粒度
              <select value={granularity} onChange={(e) => setGranularity((e.target as HTMLSelectElement).value as 'hour' | 'day')}>
                <option value="hour">小时</option>
                <option value="day">天</option>
              </select>
            </label>
            <button type="button" class="btn primary" onClick={apply}>
              应用
            </button>
            <button type="button" class="btn" onClick={() => setOpen(false)}>
              清除
            </button>
          </div>
        </div>
      )}
    </div>
  )
}
