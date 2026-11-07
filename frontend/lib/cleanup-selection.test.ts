import { createScanControl, type ScanControlView } from './scan-control.ts';
import { historicalRects } from './history-geometry.ts';
import { paintHistoryFrame, gifPacket, webmPacket } from './history-export.ts';
import type { HistoryClip, GifProgress, HistoryGifRequest } from './history-clip.ts';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readPlanResult } from './plan-result.ts';
import { readPlanProgress } from './plan-progress.ts';
import {
  createTransferCatalogue,
  type CatalogueState,
  type TransferPage,
  type TransferRecord,
} from './transfer-catalogue.ts';
import { updateCleanupSelection } from './cleanup-selection.ts';
import { addToTray, trayPercent } from './cleanup-tray.ts';
import { createNavigationRefresh } from './navigation-refresh.ts';
import { resolveZoom, zoomBox, createTreemapAnimator } from './treemap-motion.ts';
import { getIndexContext, setIndexContext, invokeIndexed } from './index-context.ts';
import { SvgSurface } from './svg-surface.ts';
import type { PaintOptions } from './map-painter.ts';
import { mapSvg, mapPng, pngDimensions } from './graphic-export.ts';
import { createStorageLayout } from './storage-map.ts';
import {
  createReportExport,
  reportSelection,
  type ReportReceipt,
  type ReportEvent,
  type ReportState,
  type ReportContext,
  type ReportSelection,
  type ReportFormat,
} from './report-export.ts';
import type { CleanupPreview } from './types';
import {
  createLiveSession,
  type LiveEvent,
  type LiveReport,
  type LiveState,
} from './live-session.ts';

function item(id: number): CleanupPreview['preview']['items'][number] {
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
  const pauseViews: ScanControlView[] = [];
  let rejectLate: (error: Error) => void = () => {};
  let nativePauseState = 'running';
  let refuseResume = false;
  let delayedQuery: Promise<unknown> | null = null;
  let resolveQuery: (state: string) => void = () => {};
  const pauseCalls: string[] = [];
  const pause = createScanControl(
    41,
    {
      query: async (id) => {
        pauseCalls.push(`query:${id}`);
        return delayedQuery ?? nativePauseState;
      },
      pause: async (id) => {
        pauseCalls.push(`pause:${id}`);
        nativePauseState = 'paused';
        return 'pause_requested';
      },
      resume: async (id) => {
        pauseCalls.push(`resume:${id}`);
        if (refuseResume) throw Error('Control unavailable');
        return 'running';
      },
    },
    (value) => pauseViews.push(value),
  );
  await pause.load();
  delayedQuery = new Promise((resolve) => {
    resolveQuery = resolve;
  });
  const supersededQuery = pause.load();
  await pause.pause();
  resolveQuery('running');
  await supersededQuery;
  delayedQuery = null;
  assert.equal(pauseViews.at(-1)?.observed, 'pause_requested');
  await pause.load();
  assert.equal(pauseViews.at(-1)?.observed, 'paused');
  await pause.resume();
  assert.equal(pauseViews.at(-1)?.observed, 'running');
  assert.deepEqual(pauseCalls, ['query:41', 'query:41', 'pause:41', 'query:41', 'resume:41']);
  await pause.pause();
  await pause.load();
  refuseResume = true;
  await pause.resume();
  assert.equal(pauseViews.at(-1)?.observed, null);
  assert.equal(pauseViews.at(-1)?.problem, 'Control unavailable');
  await pause.load();
  assert.equal(pauseViews.at(-1)?.observed, 'paused');
  refuseResume = false;
  await pause.resume();
  pause.dispose();
  const oldPause = createScanControl(
    40,
    {
      query: () =>
        new Promise((_, reject) => {
          rejectLate = reject;
        }),
      pause: async () => 'paused',
      resume: async () => 'running',
    },
    (value) => pauseViews.push(value),
  );
  const stalePauseRequest = oldPause.load();
  oldPause.dispose();
  const countBeforeLate = pauseViews.length;
  rejectLate(new Error('old scan failed'));
  await stalePauseRequest;
  assert.equal(pauseViews.length, countBeforeLate);
  const invalidPause = createScanControl(
    42,
    {
      query: async () => 'made_up_state',
      pause: async () => 'paused',
      resume: async () => 'running',
    },
    (value) => pauseViews.push(value),
  );
  await invalidPause.load();
  assert.equal(pauseViews.at(-1)?.observed, null);
  assert.equal(pauseViews.at(-1)?.problem, 'Invalid scan control response.');
  invalidPause.dispose();

  const catalogueStates: CatalogueState[] = [];
  const cataloguePages: ((page: TransferPage) => void)[] = [];
  const catalogueFailures: ((error: Error) => void)[] = [];
  const catalogue = createTransferCatalogue({
    list: () => new Promise<TransferPage>((resolve) => cataloguePages.push(resolve)),
    read: () => new Promise<TransferRecord>((_, reject) => catalogueFailures.push(reject)),
    onstate: (state) => catalogueStates.push(state),
  });
  const oldCataloguePage = catalogue.load('10');
  const currentCataloguePage = catalogue.load('5');
  cataloguePages[0]({ items: [], next_before: '9' });
  await oldCataloguePage;
  assert.equal(catalogueStates.at(-1)?.before, '5');
  assert.equal(catalogueStates.at(-1)?.loading, true);
  cataloguePages[1]({ items: [], next_before: null });
  await currentCataloguePage;
  assert.equal(catalogueStates.at(-1)?.loading, false);
  const oldRecord = catalogue.select('a'.repeat(64));
  const currentRecord = catalogue.select('b'.repeat(64));
  catalogueFailures[0](new Error('old request'));
  await oldRecord;
  assert.deepEqual(catalogueStates.at(-1)?.selection, {
    kind: 'loading',
    id: 'b'.repeat(64),
    after: null,
  });
  catalogueFailures[1](new Error('device evidence unavailable'));
  await currentRecord;
  assert.deepEqual(catalogueStates.at(-1)?.selection, {
    kind: 'failed',
    id: 'b'.repeat(64),
    after: null,
    error: 'Error: device evidence unavailable',
  });
  const disposedRecord = catalogue.select('c'.repeat(64));
  const catalogueUpdates = catalogueStates.length;
  catalogue.dispose();
  catalogueFailures[2](new Error('late response'));
  await disposedRecord;
  assert.equal(catalogueStates.length, catalogueUpdates);
  const stopped = {
    phase: 'copy_stopped',
    copied_files: 1,
    verified_bytes: '262177',
    originals: 'unchanged',
    publication: 'Uncertain',
    private_cleanup_failed: true,
    immediate_free_bytes: '0',
    full_restore_verified: false,
  };
  assert.deepEqual(readPlanResult(stopped), {
    kind: 'copy',
    copies: {
      files: 1,
      verifiedBytes: '262177',
      originals: 'unchanged',
      publication: 'Uncertain',
      privateCleanupFailed: true,
      requiresReview: true,
    },
  });
  assert.deepEqual(readPlanResult({ ...stopped, verified_bytes: '-1' }), { kind: 'unavailable' });
  assert.deepEqual(readPlanResult({ ...stopped, copied_files: 0 }), { kind: 'unavailable' });
  assert.deepEqual(
    readPlanProgress({
      kind: 'copy',
      node: 7,
      stage: 'readback',
      bytes: '9007199254740993',
      total: '9007199254740994',
    }),
    {
      kind: 'copy',
      node: 7,
      stage: 'readback',
      bytes: '9007199254740993',
      total: '9007199254740994',
    },
  );
  assert.equal(
    readPlanProgress({ kind: 'copy', node: 7, stage: 'readback', bytes: '5', total: '4' }),
    null,
  );
  const current = new Map(Array.from({ length: 499 }, (_, id) => [id, item(id)]));
  assert.equal(updateCleanupSelection(current, [item(499), item(500)], true), null);
  assert.equal(current.size, 499);
  assert.equal(current.has(499), false);
  const exact = updateCleanupSelection(current, [item(498), item(499)], true);
  assert.ok(exact);
  assert.equal(exact.size, 500);
  assert.deepEqual([...exact.keys()].slice(-2), [498, 499]);
  assert.equal(updateCleanupSelection(exact, [item(500)], true), null);
  const manualTray = addToTray([], { kind: 'manual' }, [20, 21]);
  assert.ok('groups' in manualTray);
  const mixedTray = addToTray(manualTray.groups, { kind: 'duplicates' }, [22]);
  assert.ok('groups' in mixedTray);
  assert.deepEqual(mixedTray.groups, [
    { source: { kind: 'manual' }, selection: [20, 21] },
    { source: { kind: 'duplicates' }, selection: [22] },
  ]);
  assert.ok('problem' in addToTray(mixedTray.groups, { kind: 'old_files' }, [21, 23]));
  assert.ok(
    'problem' in
      addToTray(
        mixedTray.groups,
        { kind: 'manual' },
        Array.from({ length: 498 }, (_, id) => id + 100),
      ),
  );
  assert.equal(trayPercent('100', '100'), 100);
  assert.equal(trayPercent('77', '100'), 77);
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
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      __TAURI_INTERNALS__: {
        invoke: (_command: string, args: { indexId: number }) => {
          assert.equal(args.indexId, getIndexContext());
          return new Promise<unknown>((resolve) => {
            reply.release = resolve;
          });
        },
      },
    },
  });
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
  const anchor = {
    id: 2,
    name: 'nested',
    value: 10,
    directory: true,
    x: 20,
    y: 10,
    width: 40,
    height: 20,
  };
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
  const animation = createTreemapAnimator(
    {
      now: () => now,
      request: (callback) => {
        const id = nextFrame++;
        callbacks.set(id, callback);
        return id;
      },
      cancel: (id) => {
        callbacks.delete(id);
      },
    },
    (progress) => {
      frames.push(progress);
    },
  );
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

  const liveStates: LiveState[] = [],
    liveCalls: string[] = [],
    adopted: number[] = [];
  const receivers: ((event: LiveEvent) => Promise<void>)[] = [];
  let completeStart = (_id: number) => {},
    completeThird = (_id: number) => {},
    finishAdoption = () => {};
  const pendingStart = new Promise<number>((resolve) => {
    completeStart = resolve;
  });
  const pendingThird = new Promise<number>((resolve) => {
    completeThird = resolve;
  });
  const adoption = new Promise<void>((resolve) => {
    finishAdoption = resolve;
  });
  const report = {
    reason: 'changes',
    before: { logical: '4', files: 1, measured_at: 100, complete: true, accounting_version: 1 },
    after: { logical: '14', files: 2, measured_at: 110, complete: true, accounting_version: 1 },
    net_change: '10',
  } satisfies LiveReport;
  const live = createLiveSession(
    {
      start: (_index, _concurrency, profile, receive) => {
        assert.equal(profile, 'eco');
        receivers.push(receive);
        return receivers.length === 1
          ? pendingStart
          : receivers.length === 2
            ? Promise.resolve(2)
            : pendingThird;
      },
      stop: async (id) => {
        liveCalls.push(`stop:${id}`);
      },
      acknowledge: async (id, index) => {
        liveCalls.push(`ack:${id}:${index}`);
      },
      resynchronize: async (id) => {
        liveCalls.push(`resync:${id}`);
      },
    },
    (value) => {
      liveStates.push(value);
    },
    async (event) => {
      adopted.push(event.index_id);
      await adoption;
    },
  );
  const firstLive = live.start(10, 'auto', 'eco');
  await live.stop('hidden');
  const receiveFirst = receivers[0];
  assert.ok(receiveFirst);
  await receiveFirst({ kind: 'started', id: 1 });
  assert.deepEqual(liveCalls, ['stop:1']);
  completeStart(1);
  await firstLive;
  await receiveFirst({ kind: 'ended', id: 1, error: null });
  assert.deepEqual(liveStates.at(-1), { kind: 'idle', reason: 'hidden', report: null });
  await live.start(10, 'auto', 'eco');
  const receiveSecond = receivers[1];
  assert.ok(receiveSecond);
  await receiveSecond({ kind: 'started', id: 2 });
  liveCalls.length = 0;
  const delivering = receiveSecond({
    kind: 'observed',
    id: 2,
    before_index: 10,
    index_id: 16,
    entries: 3,
    report,
  });
  assert.deepEqual(adopted, [16]);
  assert.deepEqual(liveCalls, []);
  finishAdoption();
  await delivering;
  assert.deepEqual(liveCalls, ['ack:2:16']);
  await live.resynchronize();
  assert.deepEqual(liveCalls, ['ack:2:16', 'resync:2']);
  const finalDelivery = receiveSecond({
    kind: 'observed',
    id: 2,
    before_index: 16,
    index_id: 17,
    entries: 3,
    report,
  });
  const stoppingLive = live.stop();
  await Promise.all([finalDelivery, stoppingLive]);
  await receiveSecond({ kind: 'ended', id: 2, error: null });
  assert.deepEqual(adopted, [16, 17]);
  assert.deepEqual(liveCalls, ['ack:2:16', 'resync:2', 'stop:2']);
  const thirdLive = live.start(17, 'auto', 'eco');
  const visibleStates = liveStates.length;
  await receiveSecond({ kind: 'ended', id: 2, error: 'late error' });
  assert.equal(liveStates.length, visibleStates);
  live.dispose();
  completeThird(3);
  await thirdLive;
  assert.deepEqual(liveCalls, ['ack:2:16', 'resync:2', 'stop:2', 'stop:3']);
  assert.equal(liveStates.length, visibleStates);
  const csvIndex = setIndexContext(21);
  assert.ok(csvIndex);
  const csvContext: ReportContext = {
    owner: { kind: 'index', id: csvIndex },
    requestId: '00000000-0000-4000-8000-000000000001',
  };
  const reportRequest = reportSelection(
    { parent: 3, sort: 'size', offset: 500, limit: 1, text: 'notes', min_size: 10 },
    3,
    'files',
    true,
    false,
  );
  assert.deepEqual(reportRequest, {
    items: 'files',
    filter: {
      scope: 3,
      text: 'notes',
      expression: undefined,
      extension: undefined,
      without_extension: undefined,
      min_size: 10,
      modified_before: undefined,
      cloud: undefined,
    },
    include_paths: false,
  });
  assert.deepEqual(reportSelection({ text: 'private' }, 0, 'folders', false, false), {
    items: 'folders',
    filter: { scope: 0 },
    include_paths: false,
  });
  let completeExport: (receipt: ReportReceipt) => void = () => {
    throw new Error('Missing export');
  };
  let completeRead: (event: ReportEvent | null) => void = () => {
    throw new Error('Missing progress');
  };
  const csvStates: ReportState[] = [];
  const csvCancels: string[] = [];
  const reportFormats: string[] = [];
  const csvSession = createReportExport<ReportSelection, ReportFormat>(
    {
      save: async (_context, _selection, format) => {
        reportFormats.push(format);
        return new Promise((resolve) => {
          completeExport = resolve;
        });
      },
      read: async () =>
        new Promise((resolve) => {
          completeRead = resolve;
        }),
      cancel: async (context) => {
        csvCancels.push(context.requestId);
        return csvCancels.length > 1;
      },
    },
    (state) => csvStates.push(state),
    60_000,
  );
  const savingCsv = csvSession.save(csvContext, reportRequest, 'xlsx');
  assert.deepEqual(reportFormats, ['xlsx']);
  assert.deepEqual(csvStates, [{ kind: 'choosing', cancelling: false }]);
  const progress = { rows: 1, inspected: 3, total_entries: 3 };
  completeExport({
    owner: csvContext.owner,
    request_id: csvContext.requestId,
    outcome: { kind: 'complete', path: 'owned-report.xlsx', progress },
  });
  await savingCsv;
  assert.equal(csvStates.at(-1)?.kind, 'finished');
  const settledStates = csvStates.length;
  completeRead({
    owner: csvContext.owner,
    request_id: csvContext.requestId,
    phase: { kind: 'running', id: 7, progress },
  });
  await new Promise<void>((resolve) => setImmediate(resolve));
  assert.equal(csvStates.length, settledStates);
  const cancelledContext = { ...csvContext, requestId: '00000000-0000-4000-8000-000000000002' };
  const cancellingCsv = csvSession.save(cancelledContext, reportRequest, 'pdf');
  csvSession.dispose();
  const disposedStates = csvStates.length;
  await new Promise<void>((resolve) => setImmediate(resolve));
  completeRead({
    owner: csvContext.owner,
    request_id: cancelledContext.requestId,
    phase: { kind: 'running', id: 8, progress },
  });
  await new Promise<void>((resolve) => setImmediate(resolve));
  assert.deepEqual(csvCancels, [cancelledContext.requestId, cancelledContext.requestId]);
  completeExport({
    owner: csvContext.owner,
    request_id: cancelledContext.requestId,
    outcome: { kind: 'cancelled', progress },
  });
  await cancellingCsv;
  assert.deepEqual(reportFormats, ['xlsx', 'pdf']);
  assert.equal(csvStates.length, disposedStates);
  const map = createStorageLayout([
    { id: 1, name: '<script>A & B</script>', value: 60, directory: false },
    { id: 2, name: 'Other', value: 40, directory: false },
  ]);
  const mapOptions: PaintOptions = {
    rects: map.rects,
    camera: { x: 0, y: 0, scale: 1 },
    width: 1000,
    height: 700,
    selected: 1,
    hovered: null,
    matches: null,
    color: 'type',
    maximum: 60,
    now: 0,
    detail: true,
    style: { text: '#242c29', surface: '#fcfcf9', border: '#dce1d9', dark: false },
  };
  const drawing = await mapSvg(
    { kind: 'treemap', options: mapOptions, lens: { x: 500, y: 350 } },
    (text) => ({ width: text.length * 6, actualBoundingBoxAscent: 9 }),
  );
  assert.ok(drawing.includes('<rect x="0" y="0" width="600" height="700"'));
  assert.ok(drawing.includes('&lt;script&gt;A &amp; B&lt;/script&gt;'));
  assert.ok(drawing.includes('<clipPath') && drawing.includes('clip-path="url(#map-clip-'));
  assert.ok((drawing.match(/A88 88 0 0 1 /g) ?? []).length >= 2);
  assert.ok(
    !drawing.includes('<script>') && !drawing.includes('<image') && !drawing.includes('href='),
  );
  assert.deepEqual(pngDimensions(mapOptions, 2), { width: 2000, height: 1400 });
  assert.equal(pngDimensions({ width: 8192, height: 8192 }, 1), null);
  assert.equal(pngDimensions({ width: Number.NaN, height: 1 }, 1), null);
  const stoppedImage = new AbortController();
  stoppedImage.abort();
  await assert.rejects(
    mapPng({ kind: 'treemap', options: mapOptions, lens: null }, 1, stoppedImage.signal),
    { name: 'AbortError' },
  );
  const historyFrame = {
    snapshot: 1,
    captured: 1700000000,
    observation: null,
    presence: 'present',
    kind: 'directory',
    logical: '33',
    incomplete: true,
    child_count: 2,
    entries: [
      {
        path_id: '1',
        name: '<script>notes</script>',
        kind: 'file',
        logical: '20',
        incomplete: false,
      },
      { path_id: '2', name: 'Other.bin', kind: 'file', logical: '13', incomplete: false },
    ],
    remainder: null,
  } satisfies HistoryClip['frames'][number];
  const history: HistoryClip = {
    schema_version: 1,
    root_key: 1,
    scope_fingerprint: 'a'.repeat(64),
    root_path: null,
    exclusions: null,
    exclusion_count: 1,
    paths_included: false,
    units: 'Logical path bytes',
    selection: 'Recorded root summaries',
    accounting: 'unknown',
    maximum_logical: '66',
    partial_frames: 1,
    legacy_frames: 2,
    limitations: 'Synthetic owned test data',
    fingerprint: 'b'.repeat(64),
    frames: [
      historyFrame,
      {
        ...historyFrame,
        snapshot: 2,
        logical: '66',
        incomplete: false,
        entries: historyFrame.entries.map((entry) => ({
          ...entry,
          logical: entry.logical === '20' ? '40' : '26',
        })),
      },
    ],
  };
  const historicalGeometry = historicalRects(history.frames[0], 66, 1000);
  assert.equal(
    historicalGeometry.reduce((area, rect) => area + rect.width * rect.height, 0),
    160000,
  );
  const historySurface = new SvgSurface(1000, 560, (text) => ({
    width: text.length * 6,
    actualBoundingBoxAscent: 9,
  }));
  paintHistoryFrame(historySurface, history, 0, {
    surface: '#fcfcf9',
    text: '#242c29',
    secondary: '#505c54',
  });
  const historyDrawing = historySurface.finish();
  assert.ok(historyDrawing.includes('At least 33 logical bytes'));
  assert.ok(historyDrawing.includes('Partial capture; sizes are lower bounds.'));
  assert.ok(historyDrawing.includes('Legacy accounting comparability is unverified.'));
  assert.ok(
    historyDrawing.includes('&lt;script&gt;notes&lt;/script&gt;') &&
      !historyDrawing.includes('<script>'),
  );
  const historyRequest: HistoryGifRequest = {
    clip: { snapshot_ids: [1, 2], include_paths: false },
    fingerprint: history.fingerprint,
    width: 1000,
    height: 560,
    delay_centiseconds: 100,
    looped: false,
  };
  const binary = gifPacket('owned-history-export', historyRequest, [
    new Uint8Array([1, 2]),
    new Uint8Array([3, 4, 5]),
  ]);
  const headerLength = new DataView(binary.buffer).getUint32(0, true);
  const decodedHeader: unknown = JSON.parse(
    new TextDecoder().decode(binary.slice(4, 4 + headerLength)),
  );
  assert.deepEqual(decodedHeader, {
    requestId: 'owned-history-export',
    request: historyRequest,
    lengths: [2, 3],
  });
  assert.deepEqual([...binary.slice(4 + headerLength)], [1, 2, 3, 4, 5]);
  const { looped: _looped, ...videoRequest } = historyRequest;
  const videoBinary = webmPacket(
    'owned-video-export',
    videoRequest,
    new Uint8Array([0x1a, 0x45, 0xdf, 0xa3]),
  );
  const videoHeaderLength = new DataView(videoBinary.buffer).getUint32(0, true);
  assert.deepEqual(
    JSON.parse(new TextDecoder().decode(videoBinary.slice(4, 4 + videoHeaderLength))),
    {
      requestId: 'owned-video-export',
      request: videoRequest,
    },
  );
  assert.deepEqual([...videoBinary.slice(4 + videoHeaderLength)], [0x1a, 0x45, 0xdf, 0xa3]);
  assert.throws(() => webmPacket('empty-video', videoRequest, new Uint8Array()), /export limit/);
  const videoSurface = new SvgSurface(1000, 560, (text) => ({
    width: text.length * 6,
    actualBoundingBoxAscent: 9,
  }));
  paintHistoryFrame(
    videoSurface,
    history,
    1,
    { surface: '#fcfcf9', text: '#242c29', secondary: '#505c54' },
    'webm',
  );
  const videoDrawing = videoSurface.finish();
  assert.ok(videoDrawing.includes('66 logical bytes'));
  assert.ok(
    videoDrawing.includes('WebM encoding may change colors and detail.') &&
      !videoDrawing.includes('256 colors'),
  );
  const historyOwner: ReportContext = {
    owner: { kind: 'history', fingerprint: history.fingerprint },
    requestId: '00000000-0000-4000-8000-000000000003',
  };
  let finishHistory: (value: ReportReceipt<GifProgress>) => void = () => {
    throw new Error('Missing history export');
  };
  const historyStates: ReportState<GifProgress>[] = [];
  const gifProgress: GifProgress = { stage: 'finalizing', completed_frames: 2, total_frames: 2 };
  const historySession = createReportExport<Uint8Array, 'gif', GifProgress>(
    {
      save: async () =>
        new Promise((resolve) => {
          finishHistory = resolve;
        }),
      read: async () => ({
        owner: { kind: 'index', id: csvIndex },
        request_id: historyOwner.requestId,
        phase: { kind: 'running', id: 9, progress: gifProgress },
      }),
      cancel: async () => true,
    },
    (state) => historyStates.push(state),
    60000,
  );
  const savingHistory = historySession.save(historyOwner, binary, 'gif');
  await new Promise<void>((resolve) => setImmediate(resolve));
  assert.deepEqual(historyStates, [{ kind: 'choosing', cancelling: false }]);
  finishHistory({
    owner: historyOwner.owner,
    request_id: historyOwner.requestId,
    outcome: { kind: 'complete', path: 'owned-history.gif', progress: gifProgress },
  });
  await savingHistory;
  assert.equal(historyStates.at(-1)?.kind, 'finished');
  historySession.dispose();
  const unsafeColor = new SvgSurface(10, 10, () => ({ width: 1, actualBoundingBoxAscent: 9 }));
  unsafeColor.fillStyle = 'url(https://private.invalid/image)';
  assert.throws(() => unsafeColor.fillRect(0, 0, 10, 10), /explicit RGB/);
});
