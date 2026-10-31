import { test } from 'node:test';
import assert from 'node:assert/strict';
import { updateCleanupSelection } from './cleanup-selection.ts';
import { createNavigationRefresh } from './navigation-refresh.ts';
import { getIndexContext, setIndexContext, invokeIndexed } from './index-context.ts';
import type { CleanupRulePage } from './types';

function item(id: number): CleanupRulePage['items'][number] {
  return {
    id,
    path: `C:\\Temp\\${id}.tmp`,
    logical: id,
    reclaimable: id,
    hard_link: false,
    modified: 1_000_000_000,
  };
}

test('selection, navigation and index ownership preserve user intent', async () => {
  const current = new Map(Array.from({ length: 499 }, (_, id) => [id, item(id)]));
  assert.equal(updateCleanupSelection(current, [item(499), item(500)], true), null);
  assert.equal(current.size, 499);
  assert.equal(current.has(499), false);
  const exact = updateCleanupSelection(current, [item(498), item(499)], true);
  assert.ok(exact);
  assert.equal(exact.size, 500);
  assert.deepEqual([...exact.keys()].slice(-2), [498, 499]);
  assert.equal(updateCleanupSelection(exact, [item(500)], true), null);
  const refresh = createNavigationRefresh();
  assert.equal(refresh.request(), true);
  refresh.begin(1);
  assert.equal(refresh.request(), false);
  assert.equal(refresh.request(), false);
  refresh.begin(2);
  assert.equal(refresh.request(), false);
  assert.equal(refresh.finish(1), false);
  assert.equal(refresh.finish(2), true);
  assert.equal(refresh.finish(2), false);
  assert.equal(refresh.request(), true);
  refresh.begin(3);
  assert.equal(refresh.request(), false);
  refresh.reset();
  assert.equal(refresh.finish(3), false);
  assert.equal(refresh.request(), true);
  setIndexContext(10);
  assert.equal(getIndexContext(), 10);
  assert.throws(() => setIndexContext(-1), /Invalid analysis identity/);
  assert.equal(getIndexContext(), 10);
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
  const reply: { release: ((value: unknown) => void) | null } = { release: null };
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {
    __TAURI_INTERNALS__: { invoke: (_command: string, args: { indexId: number }) => {
      assert.equal(args.indexId, getIndexContext());
      return new Promise<unknown>((resolve) => { reply.release = resolve; });
    } },
  } });
  try {
    const stale = invokeIndexed<unknown[]>('get_issues', { indexId: 999 });
    setIndexContext(11);
    assert.ok(reply.release);
    reply.release([]);
    await assert.rejects(stale, /Analysis changed/);
    const current = invokeIndexed<unknown[]>('get_issues');
    assert.ok(reply.release);
    reply.release([]);
    assert.deepEqual(await current, []);
  } finally {
    setIndexContext(null);
    if (previousWindow) Object.defineProperty(globalThis, 'window', previousWindow);
    else Reflect.deleteProperty(globalThis, 'window');
  }
  assert.equal(getIndexContext(), null);
});
