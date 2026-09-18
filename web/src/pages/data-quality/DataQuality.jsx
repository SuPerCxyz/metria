// 数据质量：来源扫描、游标、解析错误、时钟偏移和运行时观测覆盖率。

import React, { useMemo } from 'react'
import PageHeader from '../../components/common/PageHeader'
import DataTable from '../../components/tables/DataTable'
import StatusBadge from '../../components/common/StatusBadge'
import MetricCard from '../../components/cards/MetricCard'
import { EmptyState, ErrorState, LoadingSkeleton } from '../../components/feedback/Feedback'
import { api, q, rangeParams } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { useTimeRange } from '../../hooks/useTimeRange'
import { fmtDateTime, fmtPct, fmtRelative } from '../../services/format'
import { isAvailable } from '../../services/dataAvailability'

export default function DataQuality() {
  const { range } = useTimeRange()
  const params = rangeParams(range)
  const quality = useQuery(`data-quality${q(params)}`, () => api(`/data-quality${q(params)}`))
  const performance = useQuery(`quality-performance${q(params)}`, () => api(`/usage/performance${q(params)}`))

  const d = quality.data || {}
  const scan = d.source_scan || {}
  const sourceErrors = d.source_errors || []
  const alerts = d.alerts || []
  const cursorStatus = d.cursor_status || []
  const clockSkewWarnings = d.clock_skew_warnings || []
  const usageDistribution = d.usage_distribution || []
  const observationCards = useMemo(() => {
    const data = performance.data || {}
    const cards = []
    if (data.total_calls > 0) cards.push(<MetricCard key="observed-calls" label="性能调用范围" value={String(data.total_calls)} sub="当前范围模型调用" />)
    for (const [key, label] of [['ttft', '首 Token 覆盖率'], ['first_byte', '首字节覆盖率'], ['generation', '生成耗时覆盖率'], ['output_speed', '输出速度覆盖率']]) {
      const metric = data[key]
      if (metric?.count > 0 && isAvailable(metric.coverage)) cards.push(<MetricCard key={key} label={label} value={fmtPct(metric.coverage)} sub={`${metric.count} / ${data.total_calls} 次调用`} />)
    }
    return cards
  }, [performance.data])
  const hasQualityDetails = sourceErrors.length > 0 || alerts.length > 0 || cursorStatus.length > 0 || clockSkewWarnings.length > 0 || usageDistribution.length > 0

  if (quality.error && !quality.data) return <ErrorState error={quality.error} onRetry={quality.refresh} />
  if (quality.loading && !quality.data) return <LoadingSkeleton rows={8} />

  return (
    <>
      <PageHeader title="数据质量" subtitle="采集来源、解析状态与运行时观测覆盖率" />

      <div className="grid grid-cols-12 gap-6">
        {isAvailable(scan.total) && <MetricCard span="xl:col-span-3" label="来源健康" value={`${scan.healthy ?? 0} / ${scan.total}`} sub={scan.with_errors ? `${scan.with_errors} 个来源有错误` : '来源扫描状态'} />}
        {isAvailable(d.parse_warnings) && <MetricCard span="xl:col-span-3" label="解析告警" value={String(d.parse_warnings)} sub="历史来源错误记录" />}
        {isAvailable(scan.last_scan_at) && scan.last_scan_at && <MetricCard span="xl:col-span-3" label="最近扫描" value={fmtRelative(scan.last_scan_at)} sub={fmtDateTime(scan.last_scan_at)} />}
        {observationCards}
      </div>

      {quality.error && <p className="mt-3 text-sm text-amber-600 dark:text-amber-400">部分质量数据加载失败：{quality.error.message}</p>}
      {performance.error && <p className="mt-3 text-sm text-amber-600 dark:text-amber-400">性能覆盖率加载失败：{performance.error.message}</p>}

      {!hasQualityDetails && observationCards.length === 0 && <div className="mt-4 bg-white dark:bg-gray-800 rounded-2xl border border-gray-200 dark:border-gray-700/60"><EmptyState title="暂无质量异常或观测样本" desc="当前范围没有来源错误、游标异常或原生运行时观测数据。" /></div>}

      {usageDistribution.length > 0 && <QualitySection title="用量来源分布">
        <DataTable
          columns={[
            { key: 'usage_source', label: '用量来源' },
            { key: 'calls', label: '调用数', sortValue: (r) => r.calls },
            { key: 'tokens', label: '输入 Token', sortValue: (r) => r.tokens },
          ]}
          data={usageDistribution}
        />
      </QualitySection>}

      {cursorStatus.length > 0 && <QualitySection title="来源游标状态">
        <DataTable
          columns={[
            { key: 'source_id', label: 'Source', render: (r) => <span title={r.source_id} className="block max-w-[18rem] truncate">{r.source_id}</span> },
            { key: 'client_id', label: 'Agent' },
            { key: 'adapter_id', label: 'Adapter' },
            { key: 'status', label: '状态', render: (r) => <StatusBadge status={r.status} /> },
            { key: 'last_scan_at', label: '最后扫描', sortValue: (r) => r.last_scan_at, render: (r) => fmtDateTime(r.last_scan_at) },
            { key: 'last_error', label: '最近错误', hideWhenEmpty: true, render: (r) => r.last_error || '—' },
          ]}
          data={cursorStatus}
        />
      </QualitySection>}

      {sourceErrors.length > 0 && <QualitySection title="来源错误">
        <DataTable
          columns={[
            { key: 'last_seen_at', label: '最近发生', sortValue: (r) => r.last_seen_at, render: (r) => fmtDateTime(r.last_seen_at) },
            { key: 'source_id', label: 'Source' },
            { key: 'phase', label: '阶段' },
            { key: 'severity', label: '级别', render: (r) => <StatusBadge status={r.severity} /> },
            { key: 'pattern', label: '模式' },
            { key: 'sample_count', label: '次数', sortValue: (r) => r.sample_count },
          ]}
          data={sourceErrors}
        />
      </QualitySection>}

      {alerts.length > 0 && <QualitySection title="告警">
        <DataTable
          columns={[
            { key: 'last_seen_at', label: '最近发生', sortValue: (r) => r.last_seen_at, render: (r) => fmtDateTime(r.last_seen_at) },
            { key: 'source_id', label: 'Source' },
            { key: 'phase', label: '阶段' },
            { key: 'severity', label: '级别', render: (r) => <StatusBadge status={r.severity} /> },
            { key: 'pattern', label: '模式' },
          ]}
          data={alerts}
        />
      </QualitySection>}

      {clockSkewWarnings.length > 0 && <QualitySection title="时钟偏移告警">
        <DataTable
          columns={[
            { key: 'collector_id', label: '采集器' },
            { key: 'node_id', label: '节点' },
            { key: 'clock_skew_seconds', label: '偏移秒数', sortValue: (r) => Math.abs(r.clock_skew_seconds), render: (r) => `${r.clock_skew_seconds} 秒` },
            { key: 'last_heartbeat_at', label: '最后心跳', sortValue: (r) => r.last_heartbeat_at, render: (r) => fmtDateTime(r.last_heartbeat_at) },
          ]}
          data={clockSkewWarnings}
        />
      </QualitySection>}
    </>
  )
}

function QualitySection({ title, children }) {
  return (
    <section className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-4">
      <h2 className="mb-3 px-2 text-lg font-bold text-gray-800 dark:text-gray-100">{title}</h2>
      {children}
    </section>
  )
}
