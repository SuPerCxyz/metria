// 分位数近似标注的判定：字段缺失按精确处理，只认显式 true。

import assert from 'node:assert/strict'
import test from 'node:test'
import { quantileApprox } from './quantileApprox.js'

test('字段缺失（后端未部署）按精确处理，不标注', () => {
  assert.equal(quantileApprox(), false)
  assert.equal(quantileApprox(undefined), false)
  assert.equal(quantileApprox(null), false)
  assert.equal(quantileApprox({ total_calls: 3 }), false)
  assert.equal(quantileApprox(undefined, { series: [] }), false)
})

test('只有显式 quantile_approx === true 才判定为近似', () => {
  assert.equal(quantileApprox({ quantile_approx: false }), false)
  assert.equal(quantileApprox({ quantile_approx: 'true' }), false)
  assert.equal(quantileApprox({ quantile_approx: true }), true)
})

test('多个响应任一声明近似即近似（联合判定）', () => {
  assert.equal(quantileApprox({}, { quantile_approx: true }), true)
  assert.equal(quantileApprox({ quantile_approx: true }, undefined), true)
  assert.equal(quantileApprox({ quantile_approx: false }, {}), false)
})
