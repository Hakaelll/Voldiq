import { test } from 'node:test';
import assert from 'node:assert/strict';
import { updateCleanupSelection } from './cleanup-selection.ts';
import { createNavigationRefresh } from './navigation-refresh.ts';
import { resolveZoom, zoomBox, createTreemapAnimator } from './treemap-motion.ts';
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

test('frontend interactions preserve ownership and bounded motion', async () => {
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
  const anchor = { id: 2, name: 'nested', value: 10, directory: true, x: 20, y: 10, width: 40, height: 20 };
  const entering = resolveZoom(0, 2, [anchor], []);
  const leaving = resolveZoom(2, 0, [], [anchor]);
  assert.ok(entering && leaving);
  assert.equal(entering.direction, 'in');
  assert.equal(leaving.direction, 'out');
  assert.equal(resolveZoom(2, 2, [anchor], [anchor]), null);
  assert.equal(resolveZoom(1, 3, [anchor], [anchor]), null);
  assert.deepEqual(zoomBox(entering, 100, 60, 0), { x: 20, y: 10, width: 40, height: 20 });
  assert.deepEqual(zoomBox(entering, 100, 60, 0.5), { x: 2.5, y: 1.25, width: 92.5, height: 55 });
  assert.deepEqual(zoomBox(entering, 100, 60, 1), { x: 0, y: 0, width: 100, height: 60 });
  assert.deepEqual(zoomBox(leaving, 100, 60, 0), { x: 0, y: 0, width: 100, height: 60 });
  assert.deepEqual(zoomBox(leaving, 100, 60, 1), { x: 20, y: 10, width: 40, height: 20 });
  let nextFrame = 0;
  let now = 100;
  const callbacks = new Map<number, FrameRequestCallback>();
  const frames: number[] = [];
  const animation = createTreemapAnimator({
    now: () => now,
    request: (callback) => { const id = nextFrame++; callbacks.set(id, callback); return id; },
    cancel: (id) => { callbacks.delete(id); },
  }, (progress) => { frames.push(progress); });
  animation.start();
  animation.stop();
  assert.equal(callbacks.size, 0);
  assert.deepEqual(frames, [0, 1]);
  frames.length = 0;
  nextFrame = 0;
  animation.start();
  assert.deepEqual(frames, [0]);
  assert.equal(animation.running(), true);
  const firstFrame = callbacks.get(0);
  assert.ok(firstFrame);
  callbacks.delete(0);
  firstFrame(180);
  assert.deepEqual(frames, [0, 0.5]);
  const lastFrame = callbacks.get(1);
  assert.ok(lastFrame);
  callbacks.delete(1);
  lastFrame(260);
  assert.deepEqual(frames, [0, 0.5, 1]);
  assert.equal(callbacks.size, 0);
  assert.equal(animation.running(), false);
  animation.stop();
  assert.deepEqual(frames, [0, 0.5, 1]);
  now = 300;
  animation.start();
  const staleFrame = callbacks.get(2);
  assert.ok(staleFrame);
  animation.stop();
  assert.equal(callbacks.size, 0);
  assert.equal(animation.running(), false);
  const completedFrames = [...frames];
  staleFrame(380);
  assert.deepEqual(frames, completedFrames);
  assert.equal(callbacks.size, 0);
});
