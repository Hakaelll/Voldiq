import test from 'node:test';
import assert from 'node:assert/strict';
import { readPlanResult } from './plan-result.ts';

test('plan results keep recycled, returned and unknown outcomes separate', () => {
  const result = readPlanResult({
    phase: 'execution',
    total: 5,
    journal_failed: false,
    cleanup_requires_review: false,
    items: ['recycled', 'returned', 'unmoved', 'not_started', 'unknown'].map((outcome) => ({
      outcome,
    })),
  });
  assert.deepEqual(result, {
    kind: 'execution',
    recycled: 1,
    returned: 1,
    unmoved: 1,
    notStarted: 1,
    unknown: 1,
    requiresReview: true,
  });
});

test('missing, partial, unsupported and oversized plan results stay unavailable', () => {
  for (const result of [
    null,
    {},
    { phase: 'preflight' },
    { phase: 'execution', total: 0, items: [] },
    {
      phase: 'execution',
      total: 2,
      items: [{ outcome: 'recycled' }],
      journal_failed: false,
      cleanup_requires_review: false,
    },
    {
      phase: 'execution',
      total: 1,
      items: [{ outcome: 'unsupported' }],
      journal_failed: false,
      cleanup_requires_review: false,
    },
    {
      phase: 'execution',
      total: 501,
      items: Array.from({ length: 501 }, () => ({ outcome: 'recycled' })),
      journal_failed: false,
      cleanup_requires_review: false,
    },
  ]) {
    assert.deepEqual(readPlanResult(result), { kind: 'unavailable' });
  }
});
