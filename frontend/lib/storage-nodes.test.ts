import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { Entry, Overview } from './types.ts';
import { storageNodes } from './storage-nodes.ts';
import { createStorageLayout } from './storage-map.ts';

const entry = (id: number, kind: Entry['kind'], logical: number, files: number): Entry => ({
  id,
  parent: id === 0 ? null : 0,
  name: `item-${id}`,
  kind,
  logical,
  allocated: logical,
  allocation_known: true,
  files,
  modified: 900,
  hard_link: false,
  incomplete: false,
  cloud: 'unknown',
});
const directory = entry(0, 'directory', 1000, 40);
const children = { items: [entry(1, 'directory', 800, 30), entry(2, 'link', 10, 1)], total: 9 };
const branches = [{ parent: 1, items: [entry(3, 'link', 200, 1)] }];
const metrics: Overview['map_metrics'] = [
  {
    id: 0,
    logical: 100,
    allocated: 2,
    files: 4,
    allocation_known: false,
    latest_modified: 20,
    modified_known: 1,
    modified_total: 4,
  },
  {
    id: 1,
    logical: 70,
    allocated: 0,
    files: 2,
    allocation_known: false,
    latest_modified: 20,
    modified_known: 1,
    modified_total: 2,
  },
  {
    id: 2,
    logical: 10,
    allocated: 0,
    files: 1,
    allocation_known: false,
    latest_modified: null,
    modified_known: 0,
    modified_total: 1,
  },
  {
    id: 3,
    logical: 20,
    allocated: 0,
    files: 1,
    allocation_known: false,
    latest_modified: 20,
    modified_known: 1,
    modified_total: 1,
  },
];

test('all map frontiers use filtered leaf totals and exact remainders without changing native entries', () => {
  const nodes = storageNodes({
    directory,
    children,
    branches,
    metrics,
    allocated: false,
    filtered: true,
  });
  assert.ok(nodes);
  assert.deepEqual(
    nodes.map((n) => [n.id, n.value, n.files]),
    [
      [1, 70, 2],
      [2, 10, 1],
      [-1, 20, 1],
    ],
  );
  assert.equal(nodes[0].children?.[0].value, 20);
  assert.equal(nodes[0].allocationKnown, false);
  assert.equal(nodes[0].latestModified, 20);
  assert.equal(nodes[2].name, 'Other matching contents');
  assert.equal(directory.logical, 1000);
  assert.equal(children.items[0].logical, 800);
  const layout = createStorageLayout(nodes, 6);
  assert.equal(layout.total, 100);
  assert.equal(
    layout.rects.reduce((sum, r) => sum + r.value, 0),
    100,
  );
  assert.equal(layout.represented, 4);
  const allocated = storageNodes({
    directory,
    children,
    branches,
    metrics,
    allocated: true,
    filtered: true,
  });
  assert.ok(allocated);
  assert.equal(
    allocated.reduce((sum, n) => sum + n.value, 0),
    2,
  );
  assert.equal(createStorageLayout(allocated).zeroBytes, 0);
});

test('unfiltered maps retain indexed totals and incomplete filtered evidence never falls back to them', () => {
  const nodes = storageNodes({
    directory,
    children,
    branches,
    metrics,
    allocated: false,
    filtered: false,
  });
  assert.ok(nodes);
  assert.deepEqual(
    nodes.map((n) => [n.id, n.value]),
    [
      [1, 800],
      [2, 10],
      [-1, 190],
    ],
  );
  assert.equal(
    storageNodes({
      directory,
      children,
      branches,
      metrics: metrics.slice(1),
      allocated: false,
      filtered: true,
    }),
    null,
  );
  assert.equal(
    storageNodes({
      directory,
      children,
      branches,
      metrics: metrics.slice(0, 3),
      allocated: false,
      filtered: true,
    }),
    null,
  );
});
