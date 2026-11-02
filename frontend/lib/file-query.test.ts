import { test } from 'node:test';
import assert from 'node:assert/strict';
import { scopedFileQuery } from './file-query.ts';

test('advanced expressions search the complete current scope without a literal text term', () => {
  assert.deepEqual(
    scopedFileQuery({ scope: 7, ranked: false, text: '', expression: 'size>1GiB', sort: 'size' }),
    { scope: 7, text: '', expression: 'size>1GiB', sort: 'size' },
  );
  assert.deepEqual(scopedFileQuery({ scope: 7, ranked: false, expression: '' }), {
    parent: 7,
    expression: '',
  });
  assert.deepEqual(scopedFileQuery({ scope: 7, ranked: false, expression: ' \t ' }), {
    parent: 7,
    expression: ' \t ',
  });
});
