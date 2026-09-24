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
import { fmtDateTime, fmtPct, fmtRelative, fmtTokensShort, outputTokens } from '../../services/format'
import { isAvailable } from '../../services/dataAvailability'
import { activeChecks, fmtHours, hasDurationOutliers, topSeverity } from '../../services/durationOutliers'

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
  // 调用时长口径告警：旧后端或未启用时为 null / 缺失，或 checks 全为 0，此时不渲染该区块。
  const durationOutliers = d.duration_outliers
  const showDurationOutliers = hasDurationOutliers(durationOutliers)
  const durationOutlierChecks = showDurationOutliers ? activeChecks(durationOutliers.checks) : []
  const durationOutliersSeverity = showDurationOutliers ? topSeverity(durationOutliers.checks) : null
  const durationOutliersByClient = showDurationOutliers && Array.isArray(durationOutliers.by_client)
    ? durationOutliers.by_client
    : []
  const observationCards = useMemo(() => {
    const data = performance.data || {}
    const cards = []
    if (data.total_calls > 0) cards.push(<MetricCard key="observed-calls" label="性能调用范围" value={String(data.total_calls)} sub="当前范围模型调用" />)
    for (const [key, label] of [['ttft', '首个可观察输出覆盖率'], ['generation', '生成耗时覆盖率'], ['output_speed', '输出速度覆盖率']]) {
      const metric = data[key]
      if (metric?.count > 0 && isAvailable(metric.coverage)) cards.push(<MetricCard key={key} label={label} value={fmtPct(metric.coverage)} sub={`${metric.count} / ${data.total_calls} 次调用`} />)
    }
    return cards
  }, [performance.data])
  const hasQualityDetails = sourceErrors.length > 0 || alerts.length > 0 || cursorStatus.length > 0 || clockSkewWarnings.length > 0 || usageDistribution.length > 0 || showDurationOutliers

  if (quality.error && !quality.data) return <ErrorState error={quality.error} onRetry={quality.refresh} />
  if (quality.loading && !quality.data) return <LoadingSkeleton rows={8} />

  return (
    <>
      <PageHeader title="数据质量" subtitle="采集来源、解析状态与性能覆盖率" />

      <div className="grid grid-cols-12 gap-6">
        {isAvailable(scan.total) && <MetricCard span="xl:col-span-3" label="来源健康" value={`${scan.healthy ?? 0} / ${scan.total}`} sub={scan.with_errors ? `${scan.with_errors} 个来源有错误` : '来源扫描状态'} />}
        {isAvailable(d.parse_warnings) && <MetricCard span="xl:col-span-3" label="解析告警" value={String(d.parse_warnings)} sub="历史来源错误记录" />}
        {isAvailable(scan.last_scan_at) && scan.last_scan_at && <MetricCard span="xl:col-span-3" label="最近扫描" value={fmtRelative(scan.last_scan_at)} sub={fmtDateTime(scan.last_scan_at)} />}
        {observationCards}
      </div>

      {quality.error && <p className="mt-3 text-sm text-amber-600 dark:text-amber-400">部分质量数据加载失败：{quality.error.message}</p>}
      {performance.error && <p className="mt-3 text-sm text-amber-600 dark:text-amber-400">性能覆盖率加载失败：{performance.error.message}</p>}

      {!hasQualityDetails && observationCards.length === 0 && <div className="mt-4 bg-white dark:bg-gray-800 rounded-2xl border border-gray-200 dark:border-gray-700/60"><EmptyState title="暂无质量异常或性能样本" desc="当前范围没有来源错误、游标异常或可推导的性能数据。" /></div>}

      {usageDistribution.length > 0 && <QualitySection title="用量来源分布" hint="仅统计用量事件（按 usage_source 标注）的 Token；调用数见总览与性能卡。">
        <DataTable
          columns={[
            { key: 'usage_source', label: '用量来源' },
            { key: 'tokens', label: '输入 Token', sortValue: (r) => r.tokens, render: (r) => <span title={String(r.tokens ?? 0)}>{fmtTokensShort(r.tokens ?? 0)}</span> },
            { key: 'output', label: '输出 Token', sortValue: (r) => outputTokens(r), render: (r) => <span title={String(outputTokens(r))}>{fmtTokensShort(outputTokens(r))}</span> },
            { key: 'cache_read_tokens', label: '缓存读取', sortValue: (r) => r.cache_read_tokens, render: (r) => <span title={String(r.cache_read_tokens ?? 0)}>{fmtTokensShort(r.cache_read_tokens ?? 0)}</span> },
            { key: 'cache_write_tokens', label: '缓存写入', sortValue: (r) => r.cache_write_tokens, render: (r) => <span title={String(r.cache_write_tokens ?? 0)}>{fmtTokensShort(r.cache_write_tokens ?? 0)}</span> },
          ]}
          data={usageDistribution}
        />
      </QualitySection>}

      {cursorStatus.length > 0 && <QualitySection title="来源游标状态" hint="「活跃」表示最近扫描成功，不代表有新数据；是否有近期数据请看「最近数据」。">
        <DataTable
          columns={[
            { key: 'source_id', label: 'Source', render: (r) => <span title={r.source_id} className="block max-w-[18rem] truncate">{r.source_id}</span> },
            { key: 'client_id', label: 'Agent' },
            { key: 'adapter_id', label: 'Adapter' },
            { key: 'status', label: '状态', render: (r) => <StatusBadge status={r.status} /> },
            { key: 'last_event_at', label: '最近数据', sortValue: (r) => r.last_event_at, render: (r) => (r.last_event_at ? fmtDateTime(r.last_event_at) : '—') },
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

      {showDurationOutliers && (
        <QualitySection title={durationOutliers.label}>
          <div className="px-2 mb-3 flex flex-wrap items-center gap-x-8 gap-y-2">
            {durationOutliersSeverity && <StatusBadge status={durationOutliersSeverity} />}
            <Stat label="超阈值占总时长" value={fmtPct(durationOutliers.share_of_total_duration)} />
            <Stat label="超阈值累计时长" value={fmtHours(durationOutliers.over_threshold_duration_hours)} />
            <Stat label="范围内总时长" value={fmtHours(durationOutliers.total_duration_hours)} />
          </div>

          <p className="px-2 mb-2 text-xs text-gray-400 dark:text-gray-500">检查项（仅列出命中阈值的项）</p>
          <DataTable
            columns={[
              { key: 'severity', label: '级别', render: (r) => <StatusBadge status={r.severity} /> },
              { key: 'threshold', label: '阈值口径' },
              { key: 'count', label: '条数', sortValue: (r) => Number(r.count) || 0 },
            ]}
            data={durationOutlierChecks}
            emptyText="未发现超阈值调用"
          />

          {durationOutliersByClient.length > 0 && (
            <>
              <p className="px-2 mt-5 mb-2 text-xs text-gray-400 dark:text-gray-500">按 Agent 分布</p>
              <DataTable
                columns={[
                  { key: 'client_id', label: 'Agent' },
                  { key: 'count', label: '条数', sortValue: (r) => Number(r.count) || 0 },
                  { key: 'call_granularity', label: '调用粒度' },
                  { key: 'timing_source', label: '时间来源' },
                ]}
                data={durationOutliersByClient}
              />
            </>
          )}
        </QualitySection>
      )}
    </>
  )
}

function Stat({ label, value }) {
  return (
    <div>
      <p className="text-xs text-gray-400 dark:text-gray-500">{label}</p>
      <p className="text-lg font-bold text-gray-800 dark:text-gray-100 tabular-nums">{value}</p>
    </div>
  )
}

function QualitySection({ title, hint, children }) {
  return (
    <section className="mt-4 bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-4">
      <h2 className="mb-3 px-2 text-lg font-bold text-gray-800 dark:text-gray-100">{title}</h2>
      {hint && <p className="-mt-2 mb-3 px-2 text-xs text-gray-400 dark:text-gray-500">{hint}</p>}
      {children}
    </section>
  )
}
