import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileSizeLabel } from './file-size.ts';

test('file representations distinguish measured zero, unknown and partial allocation', () => {
  assert.equal(
    fileSizeLabel({ logical: 16, allocated: 0, allocation_known: false }, false),
    '16 B',
  );
  assert.equal(
    fileSizeLabel({ logical: 16, allocated: 0, allocation_known: false }, true),
    'Unknown',
  );
  assert.equal(fileSizeLabel({ logical: 16, allocated: 0, allocation_known: true }, true), '0 B');
  assert.equal(
    fileSizeLabel({ logical: 16, allocated: 8, allocation_known: false }, true),
    '8 B known · partial',
  );
  assert.equal(fileSizeLabel({ logical: 16, allocated: 8, allocation_known: true }, true), '8 B');
});
