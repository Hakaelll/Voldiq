import { test } from 'node:test';
import assert from 'node:assert/strict';
import { updateCleanupSelection } from './cleanup-selection.ts';
import type { CleanupRulePage } from './types';

function item(id: number): CleanupRulePage['items'][number] {
  return {
    id,
    path: `C:\\Temp\\${id}.tmp`,
    logical: id,
    reclaimable: id,
    hard_link: false,
    modified: 1_000_000_000,
  };
}

test('page selection preserves other pages and clears only visible files', () => {
  const current = new Map([[1, item(1)]]);
  const selected = updateCleanupSelection(current, [item(2), item(3)], true);
  assert.ok(selected);
  assert.deepEqual([...selected.keys()], [1, 2, 3]);
  assert.deepEqual([...current.keys()], [1]);
  const cleared = updateCleanupSelection(selected, [item(2), item(3)], false);
  assert.ok(cleared);
  assert.deepEqual([...cleared.keys()], [1]);
});

test('selection cap rejects a whole page without partial changes', () => {
  const current = new Map(Array.from({ length: 499 }, (_, id) => [id, item(id)]));
  assert.equal(updateCleanupSelection(current, [item(499), item(500)], true), null);
  assert.equal(current.size, 499);
  assert.equal(current.has(499), false);
  const exact = updateCleanupSelection(current, [item(498), item(499)], true);
  assert.ok(exact);
  assert.equal(exact.size, 500);
  assert.deepEqual([...exact.keys()].slice(-2), [498, 499]);
  assert.equal(updateCleanupSelection(exact, [item(500)], true), null);
});
