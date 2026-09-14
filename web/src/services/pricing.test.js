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

test('serializes model equivalence and explicit missing-price free fallback', () => {
  assert.deepEqual(
    serializeRuleDraft({
      ...EMPTY_RULE_DRAFT,
      model_pattern: 'my-custom-model',
      price_equivalent_to: 'OpenAI/GPT-5',
      price_equivalent_missing_as_free: true,
    }, { effectiveFrom: '2026-09-14T00:00:00Z' }),
    {
      model_pattern: 'my-custom-model',
      provider_pattern: '*',
      price_equivalent_to: 'openai/gpt-5',
      price_equivalent_missing_as_free: true,
      effective_from: '2026-09-14T00:00:00Z',
    },
  )
})

test('does not allow prices together with an equivalent model', () => {
  assert.throws(() => serializeRuleDraft({
    ...EMPTY_RULE_DRAFT,
    model_pattern: 'my-custom-model',
    price_equivalent_to: 'gpt-5',
    input_price: '0.08',
  }), /等价规则不需要重复填写价格/)
})
