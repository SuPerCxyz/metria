import assert from 'node:assert/strict'
import test from 'node:test'
import { canonicalModelPattern, EMPTY_RULE_DRAFT, serializeRuleDraft } from './pricing.js'

test('canonicalizes relay and free model names', () => {
  assert.equal(canonicalModelPattern('opencode-go/mimo-v2.5-free'), 'mimo-v2.5')
  assert.equal(canonicalModelPattern('mimo-v2.5'), 'mimo-v2.5')
  assert.equal(canonicalModelPattern('mimo-v2.5-freeform'), 'mimo-v2.5-freeform')
})

test('serializes decimal USD prices and zero without losing precision', () => {
  const payload = serializeRuleDraft({
    ...EMPTY_RULE_DRAFT,
    model_pattern: 'mimo-v2.5-free',
    input_price: '0.08',
    output_price: '0.25',
    cache_read_price: '0',
  })
  assert.deepEqual(payload, {
    model_pattern: 'mimo-v2.5',
    provider_pattern: '*',
    input_price: '80000',
    output_price: '250000',
    cache_read_price: '0',
  })
})

test('rejects invalid and empty price drafts', () => {
  assert.throws(() => serializeRuleDraft({ ...EMPTY_RULE_DRAFT, model_pattern: 'mimo-v2.5' }), /至少填写一项/)
  assert.throws(() => serializeRuleDraft({ ...EMPTY_RULE_DRAFT, model_pattern: 'mimo-v2.5', input_price: '-1' }), /非负美元/)
})
