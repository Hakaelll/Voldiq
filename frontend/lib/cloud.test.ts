import { test } from 'node:test';
import assert from 'node:assert/strict';
import { scopedFileQuery } from './file-query.ts';

test('cloud item filters search the complete current scope and preserve combined predicates', () => {
  assert.deepEqual(
    scopedFileQuery({ scope: 7, ranked: false, cloud: 'placeholder', text: '', sort: 'allocated' }),
    {
      scope: 7,
      cloud: 'placeholder',
      text: '',
      sort: 'allocated',
    },
  );
  assert.deepEqual(scopedFileQuery({ scope: 7, ranked: false }), { parent: 7 });
  assert.deepEqual(
    scopedFileQuery({
      scope: 7,
      ranked: false,
      cloud: 'unknown',
      extension: 'bin',
      without_extension: false,
    }),
    {
      scope: 7,
      cloud: 'unknown',
      extension: 'bin',
      without_extension: false,
    },
  );
});
