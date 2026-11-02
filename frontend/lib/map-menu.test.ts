import { test } from 'node:test';
import assert from 'node:assert/strict';
import { setIndexContext } from './index-context.ts';
import { VORONOI_GROUP_ID } from './voronoi.ts';
import { createMapMenu, mapTarget, menuCommand, menuKeyIndex, menuPosition } from './map-menu.ts';

const first = setIndexContext(1),
  second = setIndexContext(2);
const node = (id: number, directory = false) => ({ id, directory, name: 'Photos & notes' });

test('click and context actions share one native versus synthetic target decoder', () => {
  assert.deepEqual(mapTarget(0), { kind: 'entry', id: 0 });
  assert.deepEqual(mapTarget(0xffffffff), { kind: 'entry', id: 0xffffffff });
  assert.deepEqual(mapTarget(-7), { kind: 'contents', id: 5 });
  assert.deepEqual(mapTarget(-2), { kind: 'contents', id: 0 });
  assert.deepEqual(mapTarget(-1), { kind: 'list' });
  assert.deepEqual(mapTarget(VORONOI_GROUP_ID), { kind: 'list' });
  assert.equal(mapTarget(-4294967298), null);
});

test('native map menus expose only actions bound to the chosen file or folder', () => {
  const file = createMapMenu(node(7), first);
  assert.ok(file);
  assert.equal(file.name, 'Photos & notes');
  assert.equal(file.indexId, first);
  assert.deepEqual(
    file.items.map((item) => item.command),
    [
      { kind: 'inspect', id: 7 },
      { kind: 'reveal', id: 7 },
      { kind: 'copy', id: 7 },
      { kind: 'list' },
    ],
  );
  const folder = createMapMenu(node(0, true), first);
  assert.ok(folder);
  assert.deepEqual(
    folder.items.map((item) => item.command),
    [
      { kind: 'inspect', id: 0 },
      { kind: 'open', id: 0 },
      { kind: 'reveal', id: 0 },
      { kind: 'copy', id: 0 },
      { kind: 'list' },
    ],
  );
  assert.ok(createMapMenu(node(0xffffffff), first));
});

test('synthetic groups offer only their safe list or containing-folder routes', () => {
  for (const id of [-1, VORONOI_GROUP_ID]) {
    const menu = createMapMenu(node(id, true), first);
    assert.ok(menu);
    assert.deepEqual(
      menu.items.map((item) => item.command),
      [{ kind: 'list' }],
    );
  }
  const contents = createMapMenu(node(-7, true), first);
  assert.ok(contents);
  assert.deepEqual(
    contents.items.map((item) => item.command),
    [{ kind: 'open', id: 5 }, { kind: 'list' }],
  );
  const maximum = createMapMenu(node(-4294967297, true), first);
  assert.ok(maximum);
  assert.deepEqual(maximum.items[0].command, { kind: 'open', id: 0xffffffff });
  for (const id of [NaN, Infinity, 1.5, -1.5, 4294967296, -4294967298])
    assert.equal(createMapMenu(node(id), first), null);
  assert.equal(createMapMenu(node(1), null), null);
  assert.deepEqual(
    createMapMenu(null, first)?.items.map((item) => item.command),
    [{ kind: 'list' }],
  );
});

test('menu dispatch rejects stale analyses and commands outside the captured menu instance', () => {
  const menu = createMapMenu(node(9), first);
  assert.ok(menu);
  const command = menu.items[0].command;
  assert.equal(menuCommand(menu, first, command), command);
  assert.equal(menuCommand(menu, second, command), null);
  assert.equal(menuCommand(menu, null, command), null);
  assert.equal(menuCommand(menu, first, { kind: 'inspect', id: 9 }), null);
  const another = createMapMenu(node(9), first);
  assert.ok(another);
  assert.equal(menuCommand(menu, first, another.items[0].command), null);
});

test('menu keyboard movement wraps and preserves native activation keys', () => {
  assert.equal(menuKeyIndex('ArrowDown', 3, 4), 0);
  assert.equal(menuKeyIndex('ArrowUp', 0, 4), 3);
  assert.equal(menuKeyIndex('Home', 2, 4), 0);
  assert.equal(menuKeyIndex('End', 1, 4), 3);
  assert.equal(menuKeyIndex('ArrowDown', 0, 1), 0);
  assert.equal(menuKeyIndex('ArrowUp', 0, 0), null);
  for (const key of ['Enter', ' ', 'Escape', 'Tab', 'x'])
    assert.equal(menuKeyIndex(key, 0, 4), null);
});

test('menu placement uses the measured box and stays within the available viewport', () => {
  assert.deepEqual(
    menuPosition({ x: 100, y: 120 }, { width: 230, height: 188 }, { width: 1000, height: 800 }),
    { x: 100, y: 120 },
  );
  assert.deepEqual(
    menuPosition({ x: 990, y: 790 }, { width: 230, height: 188 }, { width: 1000, height: 800 }),
    { x: 762, y: 604 },
  );
  assert.deepEqual(
    menuPosition({ x: -5, y: -30 }, { width: 230, height: 188 }, { width: 1000, height: 800 }),
    { x: 8, y: 8 },
  );
  assert.deepEqual(
    menuPosition({ x: 300, y: 200 }, { width: 304, height: 184 }, { width: 320, height: 200 }),
    { x: 8, y: 8 },
  );
});
