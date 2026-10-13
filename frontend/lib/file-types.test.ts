import assert from 'node:assert/strict';
import test from 'node:test';
import { visibleFileTypes } from './file-types.ts';

test('remaining types use totals from every measured group, including groups outside the IPC page', () => {
  const distribution = visibleFileTypes({
    items: Array.from({ length: 30 }, (_, i) => ({ extension: `e${i}`, bytes: 1, count: 1 })),
    total_bytes: 80,
    total_files: 80,
  });
  assert.equal(distribution.items.length, 8);
  assert.equal(distribution.otherBytes, 72);
  assert.equal(distribution.otherFiles, 72);
  assert.equal(distribution.items.reduce((sum, item) => sum + item.bytes, 0) + distribution.otherBytes, 80);
});

test('empty and zero-byte groups retain measured counts without inventing bytes', () => {
  assert.deepEqual(visibleFileTypes({ items: [], total_bytes: 0, total_files: 0 }), {
    items: [], otherBytes: 0, otherFiles: 0,
  });
  const items = Array.from({ length: 9 }, (_, i) => ({ extension: i === 0 ? null : `e${i}`, bytes: 0, count: 2 }));
  const distribution = visibleFileTypes({ items, total_bytes: 0, total_files: 18 });
  assert.equal(distribution.items[0].extension, null);
  assert.equal(distribution.otherBytes, 0);
  assert.equal(distribution.otherFiles, 2);
  const small = visibleFileTypes({ items: items.slice(0, 1), total_bytes: 0, total_files: 2 });
  assert.equal(small.otherFiles, 0);
});
