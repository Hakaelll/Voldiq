import assert from 'node:assert/strict';
import test from 'node:test';
import { scopedFileQuery } from './file-query.ts';

test('ordinary folder browsing stays at direct children', () => {
  assert.deepEqual(
    scopedFileQuery({
      scope: 12,
      ranked: false,
      text: '',
      extension: '',
      without_extension: false,
    }),
    { parent: 12, text: '', extension: '', without_extension: false },
  );
});

test('file-type legends search their measured subtree even when search text is empty', () => {
  const missing = scopedFileQuery({
    scope: 12,
    ranked: false,
    text: '',
    extension: '',
    without_extension: true,
  });
  assert.equal(missing.scope, 12);
  assert.equal(missing.parent, undefined);
  assert.equal(missing.without_extension, true);
  const named = scopedFileQuery({ scope: 12, ranked: false, text: '', extension: 'txt' });
  assert.equal(named.scope, 12);
  assert.equal(named.parent, undefined);
  assert.equal(named.extension, 'txt');
});

test('name searches and rankings retain subtree scope and additional native filters', () => {
  const search = scopedFileQuery({
    scope: 0,
    ranked: false,
    text: 'report',
    extension: '',
    sort: 'name',
  });
  assert.equal(search.scope, 0);
  assert.equal(search.sort, 'name');
  const ranking = scopedFileQuery({
    scope: 7,
    ranked: true,
    kind: 'directory',
    min_size: 104857600,
    modified_before: 100,
    sort: 'size',
  });
  assert.deepEqual(ranking, {
    scope: 7,
    kind: 'directory',
    min_size: 104857600,
    modified_before: 100,
    sort: 'size',
  });
});
