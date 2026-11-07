import '../../test-support/setup-dom.js'

import assert from 'node:assert/strict'
import test from 'node:test'
import { Chart } from 'chart.js'
import './ChartjsConfig.jsx'

test('Chart.js registry contains the explicitly registered components', () => {
  for (const id of ['line', 'bar']) {
    assert.ok(Chart.registry.getController(id), `缺少 controller: ${id}`)
  }
  for (const id of ['line', 'point', 'bar']) {
    assert.ok(Chart.registry.getElement(id), `缺少 element: ${id}`)
  }
  for (const id of ['category', 'linear']) {
    assert.ok(Chart.registry.getScale(id), `缺少 scale: ${id}`)
  }
  for (const id of ['tooltip', 'legend', 'filler']) {
    assert.ok(Chart.registry.getPlugin(id), `缺少 plugin: ${id}`)
  }
})
