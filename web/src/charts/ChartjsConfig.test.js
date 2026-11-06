import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const source = readFileSync(new URL('./ChartjsConfig.jsx', import.meta.url), 'utf8')

test('Chart.js uses explicit registration instead of chart.js/auto', () => {
  assert.equal(source.includes("from 'chart.js/auto'"), false)
  for (const name of [
    'LineController',
    'BarController',
    'LineElement',
    'PointElement',
    'BarElement',
    'CategoryScale',
    'LinearScale',
    'Tooltip',
    'Legend',
    'Filler',
  ]) {
    assert.ok(source.includes(name), `missing Chart.js registration: ${name}`)
  }
})
