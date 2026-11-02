import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  settleSearchDraft,
  validatedSearchDraft,
  searchErrorPosition,
  type SearchDraftRequest,
  type SearchDraftState,
} from './search-editor.ts';
import { createQueryRunner } from './query-runner.ts';
import type { SearchInspection } from './types.ts';
import { setIndexContext as parseIndexId } from './index-context.ts';

test('only a validated current draft, query object and exact analysis can be applied', () => {
  const indexId = parseIndexId(7);
  const query = { text: 'report' };
  const request: SearchDraftRequest = { indexId, query, expression: 'size>1GiB' };
  const pending: SearchDraftState = { kind: 'pending', request };
  assert.equal(validatedSearchDraft(pending, request.expression, query, indexId), null);
  const valid = settleSearchDraft(pending, request, { kind: 'valid', conditions: 2 });
  assert.equal(validatedSearchDraft(valid, request.expression, query, indexId), request);
  assert.equal(validatedSearchDraft(valid, 'size>2GiB', query, indexId), null);
  assert.equal(validatedSearchDraft(valid, request.expression, { ...query }, indexId), null);
  assert.equal(validatedSearchDraft(valid, request.expression, query, parseIndexId(8)), null);
  assert.equal(validatedSearchDraft(valid, request.expression, query, null), null);
  const invalid = settleSearchDraft(pending, request, {
    kind: 'invalid',
    offset: 0,
    message: 'Invalid size.',
  });
  assert.equal(validatedSearchDraft(invalid, request.expression, query, indexId), null);
});

test('error locations translate UTF-8 offsets into readable text positions', () => {
  assert.equal(searchErrorPosition('é bad', 3), 3);
  assert.equal(searchErrorPosition('😀 bad', 5), 3);
  assert.equal(searchErrorPosition('bad', 0), 1);
  assert.equal(searchErrorPosition('é bad', 1), null);
});

test('validation keeps one active and one latest pending request without cancelling item results', async () => {
  const replies: ((value: SearchInspection) => void)[] = [];
  const lists: ((value: number) => void)[] = [];
  let itemCancels = 0;
  const items = createQueryRunner<number, number>('file_list', {
    run: () => new Promise((resolve) => lists.push(resolve)),
    cancel: async () => {
      itemCancels++;
    },
  });
  const validator = createQueryRunner<SearchInspection, string, 'search_validation'>(
    'search_validation',
    { run: () => new Promise((resolve) => replies.push(resolve)), cancel: async () => {} },
  );
  const page = items.request(1).result;
  const first = validator.request('a').result;
  await new Promise<void>((resolve) => setImmediate(resolve));
  const second = validator.request('b').result;
  const latest = validator.request('c').result;
  assert.equal(await first, null);
  assert.equal(await second, null);
  assert.equal(replies.length, 1);
  replies[0]({ kind: 'valid', conditions: 1 });
  await new Promise<void>((resolve) => setImmediate(resolve));
  assert.equal(replies.length, 2);
  replies[1]({ kind: 'valid', conditions: 3 });
  assert.deepEqual(await latest, { kind: 'valid', conditions: 3 });
  assert.equal(itemCancels, 0);
  lists[0](42);
  assert.equal(await page, 42);
});

test('old replies cannot settle another request even when its draft text repeats', () => {
  const query = {};
  const first: SearchDraftRequest = { indexId: parseIndexId(7), query, expression: 'type:file' };
  const repeated = { ...first };
  const current: SearchDraftState = { kind: 'pending', request: repeated };
  assert.equal(settleSearchDraft(current, first, { kind: 'valid', conditions: 1 }), current);
  const failed = settleSearchDraft(current, repeated, { kind: 'failed', message: 'Unavailable' });
  assert.equal(validatedSearchDraft(failed, repeated.expression, query, repeated.indexId), null);
});
