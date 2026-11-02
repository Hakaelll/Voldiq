import { test } from 'node:test';
import assert from 'node:assert/strict';
import { setIndexContext } from './index-context.ts';
import { createStorageLayout } from './storage-map.ts';
import type { WeightedNode } from './treemap.ts';
import {
  emptyExploration,
  explorationIds,
  updateExploration,
  selectionOperation,
  projectedSelection,
} from './exploration.ts';

const first = setIndexContext(1),
  second = setIndexContext(2);

test('exploration replaces, adds and toggles unique IDs without mutating the previous selection', () => {
  const initial = emptyExploration(first);
  const one = updateExploration(initial, first, [3, 4], 'replace');
  assert.equal(one.kind, 'accepted');
  if (one.kind !== 'accepted') return;
  const two = updateExploration(one.state, first, [4, 5, 5], 'toggle');
  assert.equal(two.kind, 'accepted');
  if (two.kind !== 'accepted') return;
  assert.deepEqual([...two.state.ids], [3, 5]);
  assert.deepEqual([...one.state.ids], [3, 4]);
  assert.equal(initial.ids.size, 0);
  const three = updateExploration(two.state, first, [5, 6], 'add');
  assert.equal(three.kind, 'accepted');
  if (three.kind === 'accepted') assert.deepEqual([...three.state.ids], [3, 5, 6]);
});

test('exploratory IDs are bound to the exact current index and invalid updates are atomic', () => {
  const populated = updateExploration(emptyExploration(first), first, [10], 'replace');
  assert.equal(populated.kind, 'accepted');
  if (populated.kind !== 'accepted') return;
  assert.deepEqual([...explorationIds(populated.state, second)], []);
  const moved = updateExploration(populated.state, second, [20], 'add');
  assert.equal(moved.kind, 'accepted');
  if (moved.kind === 'accepted') assert.deepEqual([...moved.state.ids], [20]);
  for (const ids of [[-1], [Number.MIN_SAFE_INTEGER], [3.5], [NaN], [4294967296]])
    assert.equal(updateExploration(populated.state, first, ids, 'add').kind, 'invalid');
  assert.equal(updateExploration(populated.state, null, [1], 'replace').kind, 'invalid');
  assert.deepEqual([...populated.state.ids], [10]);
});

test('the 500-item exploration limit applies atomically to the resulting set', () => {
  const full = updateExploration(
    emptyExploration(first),
    first,
    Array.from({ length: 500 }, (_, i) => i),
    'replace',
  );
  assert.equal(full.kind, 'accepted');
  if (full.kind !== 'accepted') return;
  assert.equal(updateExploration(full.state, first, [500], 'add').kind, 'limit');
  const unchanged = updateExploration(full.state, first, [499, 499], 'add');
  assert.equal(unchanged.kind, 'accepted');
  if (unchanged.kind === 'accepted') assert.equal(unchanged.state.ids.size, 500);
  const exchange = updateExploration(full.state, first, [0, 500], 'toggle');
  assert.equal(exchange.kind, 'accepted');
  if (exchange.kind === 'accepted') {
    assert.equal(exchange.state.ids.size, 500);
    assert.equal(exchange.state.ids.has(0), false);
    assert.equal(exchange.state.ids.has(500), true);
  }
  assert.equal(full.state.ids.has(0), true);
  assert.equal(full.state.ids.has(500), false);
});

test('selection modifiers and hierarchy projection retain explicit versus represented IDs', () => {
  assert.equal(selectionOperation({ ctrlKey: true, metaKey: false, shiftKey: true }), 'toggle');
  assert.equal(selectionOperation({ ctrlKey: false, metaKey: true, shiftKey: false }), 'toggle');
  assert.equal(selectionOperation({ ctrlKey: false, metaKey: false, shiftKey: true }), 'add');
  assert.equal(selectionOperation({ ctrlKey: false, metaKey: false, shiftKey: false }), 'replace');
  const parents = new Map<number, number | null>([
    [1, null],
    [2, 1],
    [3, 1],
    [-3, 1],
    [4, null],
  ]);
  assert.deepEqual([...projectedSelection([2, 3, -3, 4], parents, new Set([1]), [])], [2, 3, -3]);
  assert.deepEqual([...projectedSelection([2, 3, -3, 4], parents, new Set([4]), [])], [4]);
  assert.deepEqual(
    [...projectedSelection([2, 3, -3, 4], parents, new Set([9]), [9, 0])],
    [2, 3, -3, 4],
  );
  assert.deepEqual([...projectedSelection([2], new Map([[2, 2]]), new Set([3]), [])], []);
});

test('a folder selection projects through loaded depth and scope while excluding sibling scopes', () => {
  const nodes: WeightedNode[] = [
    {
      id: 2,
      name: 'Photos',
      value: 800,
      directory: true,
      files: 8,
      children: [
        { id: 3, name: 'first.jpg', value: 400, directory: false },
        { id: 4, name: 'second.jpg', value: 200, directory: false },
      ],
    },
    { id: 5, name: 'other.zip', value: 200, directory: false },
  ];
  const selected = updateExploration(emptyExploration(first), first, [2], 'replace');
  assert.equal(selected.kind, 'accepted');
  if (selected.kind !== 'accepted') return;
  const project = (tree: WeightedNode[], depth: 1 | 6, ancestors: number[] = []) => {
    const map = createStorageLayout(tree, depth);
    return [
      ...projectedSelection(
        map.rects.map((rect) => rect.id),
        map.parents,
        selected.state.ids,
        ancestors,
      ),
    ].sort((a, b) => a - b);
  };
  assert.deepEqual(project(nodes, 1), [2]);
  assert.deepEqual(project(nodes, 6), [-4, 3, 4]);
  assert.deepEqual(project(nodes[0].children ?? [], 6, [2, 0]), [3, 4]);
  assert.deepEqual(project([nodes[1]], 6, [7, 0]), []);
  assert.deepEqual([...selected.state.ids], [2]);
});
