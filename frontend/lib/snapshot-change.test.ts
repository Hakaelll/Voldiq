import { test } from 'node:test';
import assert from 'node:assert/strict';
import { snapshotChangeBars } from './snapshot-change.ts';

const changes = {
  added: { files: 2, logical: 100 },
  changed: { files: 3, delta: -25 },
  removed: { files: 1, logical: 75 },
  newly_observed: { files: 99, logical: 99999 },
  unobserved: { files: 100, logical: 100000 },
};

test('comparison bars preserve signed file totals and exclude uncertain coverage', () => {
  const bars = snapshotChangeBars(changes);
  assert.deepEqual(
    bars.map((row) => [row.delta, row.files, row.start, row.width]),
    [
      [100, 2, 50, 50],
      [-25, 3, 37.5, 12.5],
      [-75, 1, 12.5, 37.5],
    ],
  );
  assert.deepEqual(snapshotChangeBars({ ...changes, changed: { files: 3, delta: 50 } })[1], {
    kind: 'changed',
    label: 'Changed',
    delta: 50,
    files: 3,
    start: 50,
    width: 25,
  });
});

test('identical captures have no bars and finite centered geometry', () => {
  const bars = snapshotChangeBars({
    ...changes,
    added: { files: 0, logical: 0 },
    changed: { files: 2, delta: 0 },
    removed: { files: 0, logical: 0 },
  });
  assert.deepEqual(
    bars.map((row) => [row.start, row.width]),
    [
      [50, 0],
      [50, 0],
      [50, 0],
    ],
  );
  assert.equal(bars[1].files, 2);
});
