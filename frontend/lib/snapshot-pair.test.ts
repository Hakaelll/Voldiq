import { test } from 'node:test';
import assert from 'node:assert/strict';
import { updateSnapshotPair } from './snapshot-pair.ts';

const older = { id: 1, root_key: 10, captured: 100 };
const newer = { id: 2, root_key: 10, captured: 200 };

test('snapshot pair orders captures chronologically and supports clearing either capture', () => {
  assert.deepEqual(updateSnapshotPair([newer], older, true), [older, newer]);
  assert.deepEqual(updateSnapshotPair([older, newer], older, false), [newer]);
  assert.deepEqual(updateSnapshotPair([older], older, false), []);
  assert.deepEqual(updateSnapshotPair([older, newer], newer, true), [older, newer]);
  assert.deepEqual(updateSnapshotPair([newer], { ...older, captured: 200 }, true), [
    { ...older, captured: 200 },
    newer,
  ]);
});

test('snapshot pair rejects unrelated roots and a third capture without changing its input', () => {
  const current = [older, newer];
  assert.equal(updateSnapshotPair(current, { id: 3, root_key: 10, captured: 300 }, true), null);
  assert.equal(updateSnapshotPair([older], { id: 4, root_key: 20, captured: 300 }, true), null);
  assert.deepEqual(current, [older, newer]);
});
