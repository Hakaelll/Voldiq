import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cloudLabel, entryKindLabel, parseCloudFilter } from './cloud.ts';
import { scopedFileQuery } from './file-query.ts';

test('cloud controls preserve unknown, no-recall and non-placeholder distinctions', () => {
  assert.equal(cloudLabel('unknown'), 'Unknown');
  assert.equal(cloudLabel('not_placeholder'), 'Not a detected placeholder');
  assert.equal(cloudLabel('file_no_recall_flag'), 'File placeholder · no recall flag');
  assert.equal(cloudLabel('file_recall_required'), 'File placeholder · recall required');
  assert.equal(cloudLabel('directory_no_recall_flag'), 'Folder placeholder · no recall flag');
  assert.equal(cloudLabel('directory_recall_required'), 'Folder placeholder · recall required');
  assert.equal(
    entryKindLabel({ kind: 'link', cloud: 'file_recall_required', hard_link: false }),
    'File placeholder',
  );
  assert.equal(
    entryKindLabel({ kind: 'link', cloud: 'directory_no_recall_flag', hard_link: false }),
    'Folder placeholder',
  );
  assert.equal(
    entryKindLabel({ kind: 'link', cloud: 'unknown', hard_link: false }),
    'Reparse point',
  );
  assert.equal(
    entryKindLabel({ kind: 'file', cloud: 'not_placeholder', hard_link: true }),
    'File · hard link',
  );
  assert.equal(parseCloudFilter(''), null);
  for (const value of [
    'placeholder',
    'recall_required',
    'no_recall_flag',
    'not_placeholder',
    'unknown',
  ]) {
    assert.equal(parseCloudFilter(value), value);
  }
  assert.equal(parseCloudFilter('online_only'), undefined);
});

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
