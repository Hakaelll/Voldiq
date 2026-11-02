import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  translationDraft,
  translationIsCurrent,
  settleTranslation,
  type TranslationRequest,
  type TranslationState,
} from './search-translation.ts';
import { setIndexContext as parseIndexId } from './index-context.ts';

test('a translated proposal needs the exact current draft, filters and analysis before use', () => {
  const request: TranslationRequest = {
    source: 'files larger than 1GiB',
    draft: '',
    query: {},
    indexId: parseIndexId(1),
  };
  const state: TranslationState = {
    kind: 'ready',
    request,
    proposal: { expression: 'type:file size>1GiB', conditions: 2, unparsed: [] },
  };
  assert.equal(translationDraft(state, request, false), 'type:file size>1GiB');
  for (const context of [
    { ...request, source: 'folders' },
    { ...request, draft: 'name:other' },
    { ...request, query: {} },
    { ...request, indexId: parseIndexId(2) },
    { ...request, indexId: null },
  ]) {
    assert.equal(translationIsCurrent(state, context), false);
    assert.equal(translationDraft(state, context, true), null);
  }
  assert.equal(translationDraft({ kind: 'pending', request }, request, true), null);
  assert.equal(
    translationDraft({ kind: 'failed', request, message: 'Unavailable' }, request, true),
    null,
  );
});

test('unparsed intent needs explicit partial acceptance and an empty interpretation cannot be used', () => {
  const request: TranslationRequest = {
    source: 'vídeos antiguos',
    draft: '',
    query: {},
    indexId: parseIndexId(1),
  };
  const state: TranslationState = {
    kind: 'ready',
    request,
    proposal: {
      expression: 'type:file category:video',
      conditions: 2,
      unparsed: [{ start: 8, end: 16, text: 'antiguos' }],
    },
  };
  assert.equal(translationDraft(state, request, false), null);
  assert.equal(translationDraft(state, request, true), 'type:file category:video');
  assert.equal(
    translationDraft({ ...state, proposal: { ...state.proposal, expression: '' } }, request, true),
    null,
  );
});

test('late repeated-text replies and dismissed translation requests cannot replace a newer proposal', () => {
  const first: TranslationRequest = {
    source: 'files',
    draft: '',
    query: {},
    indexId: parseIndexId(1),
  };
  const second = { ...first };
  const state: TranslationState = { kind: 'pending', request: second };
  const outcome = {
    kind: 'ready',
    proposal: { expression: 'type:file', conditions: 1, unparsed: [] },
  } satisfies Parameters<typeof settleTranslation>[2];
  assert.equal(settleTranslation(state, first, outcome), state);
  assert.equal(settleTranslation(null, second, outcome), null);
  assert.equal(settleTranslation(state, second, outcome)?.kind, 'ready');
});
