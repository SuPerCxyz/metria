// 设置页：模型价格 / 数据保留 / 节点接入 / 系统设置。

import React, { useState } from 'react'
import PageHeader from '../../components/common/PageHeader'
import { ErrorState, LoadingSkeleton } from '../../components/feedback/Feedback'
import { api } from '../../services/api'
import { useQuery } from '../../hooks/useQuery'
import { fmtUsd } from '../../services/format'

const TABS = ['模型价格', '数据保留', '节点接入', '系统设置']

export default function Settings() {
  const [tab, setTab] = useState('模型价格')
  const rules = useQuery('pricing-rules', () => api('/pricing/rules'))

  return (
    <>
      <PageHeader title="设置" subtitle="价格配置与系统设置" />
      <div className="inline-flex rounded-lg bg-gray-100 dark:bg-gray-700/40 p-0.5 mb-6 flex-wrap">
        {TABS.map((t) => (
          <button key={t} type="button" onClick={() => setTab(t)} className={`px-4 py-2 text-sm font-medium rounded-md ${tab === t ? 'bg-white dark:bg-gray-600 shadow-xs text-gray-800 dark:text-gray-100' : 'text-gray-500 dark:text-gray-400'}`}>
            {t}
          </button>
        ))}
      </div>

      {tab === '模型价格' && (
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-4">价格规则</h2>
          {rules.error && <ErrorState error={rules.error} onRetry={rules.refresh} />}
          {rules.loading && <LoadingSkeleton rows={5} />}
          {!rules.loading && !rules.error && (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-gray-200 dark:border-gray-700/60">
                    <th className="px-3 py-2.5 text-left text-xs font-semibold text-gray-500 dark:text-gray-400">模型匹配</th>
                    <th className="px-3 py-2.5 text-left text-xs font-semibold text-gray-500 dark:text-gray-400">输入/百万</th>
                    <th className="px-3 py-2.5 text-left text-xs font-semibold text-gray-500 dark:text-gray-400">输出/百万</th>
                    <th className="px-3 py-2.5 text-left text-xs font-semibold text-gray-500 dark:text-gray-400">来源</th>
                  </tr>
                </thead>
                <tbody>
                  {(rules.data?.rules || []).map((r) => (
                    <tr key={r.id} className="border-b border-gray-100 dark:border-gray-800">
                      <td className="px-3 py-3 text-gray-700 dark:text-gray-200">{r.model_pattern}</td>
                      <td className="px-3 py-3 text-gray-600 dark:text-gray-300 tabular-nums">{r.input_price != null ? fmtUsd(r.input_price) : '—'}</td>
                      <td className="px-3 py-3 text-gray-600 dark:text-gray-300 tabular-nums">{r.output_price != null ? fmtUsd(r.output_price) : '—'}</td>
                      <td className="px-3 py-3"><span className="text-xs px-2 py-0.5 rounded-full bg-gray-100 dark:bg-gray-700/40 text-gray-600 dark:text-gray-300">{r.source}</span></td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {tab === '数据保留' && (
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-2">数据保留策略</h2>
          <p className="text-sm text-gray-500 dark:text-gray-400">详细保留策略见运维文档（docs/operations.md）。备份与恢复通过 CLI：<code className="text-xs bg-gray-100 dark:bg-gray-700 px-1.5 py-0.5 rounded">metria backup / metria restore</code></p>
        </div>
      )}

      {tab === '节点接入' && (
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-2">节点接入</h2>
          <p className="text-sm text-gray-500 dark:text-gray-400">通过 <code className="text-xs bg-gray-100 dark:bg-gray-700 px-1.5 py-0.5 rounded">metria agent</code> 采集节点数据，详见部署文档。</p>
        </div>
      )}

      {tab === '系统设置' && (
        <div className="bg-white dark:bg-gray-800 shadow-xs rounded-2xl border border-gray-200 dark:border-gray-700/60 p-6">
          <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100 mb-2">系统信息</h2>
          <p className="text-sm text-gray-500 dark:text-gray-400">Metria Hub · AI 编程 Agent 用量监控平台。</p>
        </div>
      )}
    </>
  )
}
