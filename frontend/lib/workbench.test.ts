import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  detachedQuery,
  restoreWorkspace,
  workspaceContextIsCurrent,
  type ViewSpec,
} from './workbench.ts';
import { setIndexContext } from './index-context.ts';
import { scopedFileQuery } from './file-query.ts';
import { paletteCommands, filterCommands, paletteFocusIndex } from './command-palette.ts';

const spec: ViewSpec = {
  query: {
    expression: 'category:video',
    text: 'archive',
    extension: 'mp4',
    cloud: 'unknown',
    sort: 'name',
    modified_before: 1709251200,
  },
  scope: 'current_folder',
  view: 'explorer',
  representation: 'voronoi',
  metric: 'logical',
  depth: 4,
  color: 'age',
};
test('saved filters detach transient index and page values without mutating or losing conditions', () => {
  const query = { ...spec.query, scope: 27, offset: 500, limit: 20 };
  assert.deepEqual(detachedQuery(query), spec.query);
  assert.equal(query.scope, 27);
  assert.deepEqual(scopedFileQuery({ scope: 7, ranked: false, text: '', expression: 'size>1GiB', sort: 'size' }), { scope: 7, text: '', expression: 'size>1GiB', sort: 'size' });
  assert.deepEqual(scopedFileQuery({ scope: 7, ranked: false, expression: ' \t ' }), { parent: 7, expression: ' \t ' });
  assert.equal(
    scopedFileQuery({
      ...detachedQuery(spec.query),
      scope: restoreWorkspace(spec, 27).scope,
      ranked: false,
    }).scope,
    27,
  );
  assert.deepEqual(
    scopedFileQuery({ scope: restoreWorkspace({ ...spec, query: {} }, 27).scope, ranked: false }),
    { parent: 27 },
  );
  assert.deepEqual(restoreWorkspace(spec, 27), {
    view: 'explorer',
    scope: 27,
    text: 'archive',
    expression: 'category:video',
    extension: 'mp4',
    withoutExtension: false,
    cloud: 'unknown',
    minSize: 0,
    largeKind: 'file',
    before: '2024-03-01',
    sort: 'name',
    metric: 'logical',
    depth: 4,
    color: 'age',
    mapMode: 'voronoi',
    explorerMap: true,
    fileRepresentation: 'table',
  });
  const restored = restoreWorkspace(
    {
      ...spec,
      scope: 'analysis_root',
      representation: 'grid',
      metric: 'allocated',
      query: { sort: 'allocated' },
    },
    27,
  );
  assert.equal(restored.scope, 0);
  assert.equal(restored.sort, 'size');
  assert.equal(restored.fileRepresentation, 'grid');
  assert.equal(restored.mapMode, null);
  const large = restoreWorkspace(
    {
      ...spec,
      view: 'large',
      representation: 'table',
      query: { min_size: 1572864, kind: 'directory' },
    },
    27,
  );
  assert.equal(large.minSize, 1.5);
  assert.equal(large.largeKind, 'directory');
});
test('workspace loads require the exact current catalogue, analysis, scope, query and interaction', () => {
  const request = { indexId: setIndexContext(1), scope: 27, query: {}, owner: 8, revision: '3' };
  assert.equal(workspaceContextIsCurrent(request, { ...request }), true);
  for (const current of [
    { ...request, indexId: setIndexContext(2) },
    { ...request, indexId: null },
    { ...request, scope: 0 },
    { ...request, query: {} },
    { ...request, owner: 9 },
    { ...request, revision: '4' },
  ])
    assert.equal(workspaceContextIsCurrent(request, current), false);
});
test('the palette finds saved filters and explicit favorite scans, skips disabled commands and wraps keys', () => {
  const commands = paletteCommands(
    {
      revision: '3',
      views: [{ id: 7, name: 'Video archives', spec, updated: 0 }],
      favorites: [{ id: 8, name: 'Projects', path: 'C:\\Projects', updated: 0 }],
    },
    true,
    false,
  );
  const saved = filterCommands(commands, 'video current');
  assert.equal(saved.length, 1);
  assert.deepEqual(saved[0].action, { kind: 'load', reference: { id: 7, revision: '3' } });
  const favorite = filterCommands(commands, 'scan projects');
  assert.equal(favorite.length, 1);
  assert.deepEqual(favorite[0].action, {
    kind: 'scan',
    reference: { id: 8, revision: '3' },
    path: 'C:\\Projects',
  });
  const disabled = paletteCommands(null, false, true);
  assert.equal(disabled[0].disabled, true);
  assert.equal(paletteFocusIndex('ArrowDown', -1, disabled), 1);
  assert.equal(paletteFocusIndex('Home', 5, disabled), 1);
  assert.equal(paletteFocusIndex('ArrowUp', 1, disabled), disabled.length - 1);
  assert.equal(paletteFocusIndex('ArrowDown', disabled.length - 1, disabled), 1);
  assert.equal(paletteFocusIndex('Escape', 1, disabled), null);
  assert.equal(paletteFocusIndex('End', 0, []), null);
});
