import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createStorageLayout } from './storage-map.ts';
import { computeVoronoi, voronoiHit, VORONOI_GROUP_ID, VORONOI_CELL_LIMIT } from './voronoi.ts';
import type { Point } from './disk-city.ts';
import type { WeightedNode } from './treemap.ts';

const file = (id: number, value: number): WeightedNode => ({
  id,
  value,
  name: `${id}.bin`,
  directory: false,
});
const deterministic = { expired: () => false };
const area = (points: Point[]): number =>
  Math.abs(
    points.reduce((sum, p, i) => {
      const next = points[(i + 1) % points.length];
      return sum + p.x * next.y - p.y * next.x;
    }, 0),
  ) / 2;
function intersection(first: Point[], second: Point[]): Point[] {
  let result = first;
  for (let k = 0; k < second.length; k++) {
    const a = second[k],
      b = second[(k + 1) % second.length],
      next: Point[] = [];
    const side = (p: Point) => (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    for (let i = 0; i < result.length; i++) {
      const p = result[i],
        q = result[(i + 1) % result.length],
        s = side(p),
        t = side(q);
      if (s >= 0) next.push(p);
      if (s >= 0 !== t >= 0) {
        const ratio = s / (s - t);
        next.push({ x: p.x + ratio * (q.x - p.x), y: p.y + ratio * (q.y - p.y) });
      }
    }
    result = next;
  }
  return result;
}

test('weighted Voronoi retains the exact common frontier and measures its actual area errors', () => {
  const source = createStorageLayout([
    { ...file(1, 900), directory: true, children: [file(2, 450), file(3, 300)] },
    file(4, 100),
  ]);
  const result = computeVoronoi(source.rects, source.total, deterministic);
  assert.deepEqual(
    result.cells.flatMap((cell) => cell.members).sort((a, b) => a - b),
    [-3, 2, 3, 4],
  );
  assert.equal(
    result.cells.reduce((sum, cell) => sum + cell.node.value, 0),
    1000,
  );
  let error = 0,
    maximum = 0;
  for (const cell of result.cells) {
    const measured = area(cell.polygon) / 700000,
      target = cell.node.value / 1000;
    assert.ok(Math.abs(measured - target) / target < 0.01);
    assert.ok(Math.abs(measured - cell.area / 700000) < 1e-10);
    error += Math.abs(measured - target) / 2;
    maximum = Math.max(maximum, Math.abs(measured - target) / target);
  }
  assert.ok(Math.abs(error - (result.totalError ?? Infinity)) < 1e-10);
  assert.ok(Math.abs(maximum - (result.maximumError ?? Infinity)) < 1e-10);
  assert.deepEqual(
    result,
    computeVoronoi([...source.rects].reverse(), source.total, deterministic),
  );
});

test('Voronoi polygons tile the bounded canvas without overlapping and pick by their shared geometry', () => {
  const source = createStorageLayout(
    [20, 15, 13, 12, 10, 8, 7, 6, 5, 4].map((value, id) => file(id, value)),
  );
  const result = computeVoronoi(source.rects, source.total, deterministic);
  assert.ok(
    Math.abs(result.cells.reduce((sum, cell) => sum + area(cell.polygon), 0) - 700000) < 1e-5,
  );
  for (const cell of result.cells) {
    assert.ok(cell.polygon.length >= 3);
    for (const p of cell.polygon)
      assert.ok(
        Number.isFinite(p.x) &&
          Number.isFinite(p.y) &&
          p.x >= -1e-8 &&
          p.x <= 1000 + 1e-8 &&
          p.y >= -1e-8 &&
          p.y <= 700 + 1e-8,
      );
    assert.equal(voronoiHit(result, cell.anchor)?.node.id, cell.node.id);
    for (const other of result.cells)
      if (cell !== other) assert.ok(area(intersection(cell.polygon, other.polygon)) < 1e-5);
  }
  assert.equal(voronoiHit(result, { x: -1, y: 350 }), null);
  assert.equal(voronoiHit(result, { x: 500, y: 701 }), null);
});

test('bounded Voronoi aggregation retains every source membership and byte exactly once', () => {
  const source = createStorageLayout(Array.from({ length: 250 }, (_, id) => file(id, 1)));
  const result = computeVoronoi(source.rects, source.total, deterministic);
  assert.equal(result.cells.length, VORONOI_CELL_LIMIT);
  assert.equal(result.groupedEntries, 131);
  assert.equal(result.sourceEntries, 250);
  const group = result.cells.find((cell) => cell.node.id === VORONOI_GROUP_ID);
  assert.ok(group);
  assert.equal(group.node.value, 131);
  assert.deepEqual(
    group.members,
    Array.from({ length: 131 }, (_, i) => i + 119),
  );
  assert.deepEqual(
    result.cells.flatMap((cell) => cell.members).sort((a, b) => a - b),
    Array.from({ length: 250 }, (_, i) => i),
  );
  assert.equal(
    result.cells.reduce((sum, cell) => sum + cell.node.value, 0),
    250,
  );
  assert.ok(result.maximumError !== null && result.maximumError < 0.01);
  assert.ok(result.clips <= 1_500_000 && result.iterations <= 48);
});

test('dominant and tiny entries retain measurable areas, while an unsupported range is explicit', () => {
  const source = createStorageLayout([file(1, 1e12), file(2, 1)]);
  const result = computeVoronoi(source.rects, source.total, deterministic);
  assert.equal(result.cells.length, 2);
  for (const cell of result.cells) {
    assert.ok(area(cell.polygon) > 0);
    assert.equal(voronoiHit(result, cell.anchor)?.node.id, cell.node.id);
    assert.ok(
      Math.abs(area(cell.polygon) / ((700000 * cell.node.value) / source.total) - 1) < 0.05,
    );
  }
  const unsupported = createStorageLayout([file(1, 1e19), file(2, 1)]);
  assert.throws(
    () => computeVoronoi(unsupported.rects, unsupported.total, deterministic),
    /precision/i,
  );
});

test('Voronoi group identity stays distinct from existing root and remainder aggregates', () => {
  const source = createStorageLayout([
    { ...file(-1, 200), directory: true, category: 'remainder' },
    ...Array.from({ length: 249 }, (_, i) => file(i + 1, 1)),
  ]);
  const result = computeVoronoi(source.rects, source.total, deterministic);
  assert.equal(result.cells.filter((cell) => cell.node.id === -1).length, 1);
  assert.equal(result.cells.filter((cell) => cell.node.id === VORONOI_GROUP_ID).length, 1);
  assert.equal(new Set(result.cells.flatMap((cell) => cell.members)).size, 250);
  assert.equal(
    result.cells.reduce((sum, cell) => sum + cell.node.value, 0),
    449,
  );
});

test('Voronoi work and expiry limits never expose an inaccurate incomplete diagram', () => {
  const source = createStorageLayout([file(1, 8), file(2, 1), file(3, 1)]);
  assert.throws(
    () => computeVoronoi(source.rects, source.total, { clips: 0, expired: () => false }),
    /budget/i,
  );
  assert.throws(
    () => computeVoronoi(source.rects, source.total, { expired: () => true }),
    /budget/i,
  );
  assert.throws(
    () => computeVoronoi(source.rects, source.total, { iterations: 0, expired: () => false }),
    /precision/i,
  );
  assert.throws(() => computeVoronoi(source.rects, source.total, { clips: NaN }), /budget/i);
  assert.throws(() => computeVoronoi(source.rects, source.total, { iterations: 1.5 }), /budget/i);
  const equal = createStorageLayout([file(1, 1), file(2, 1)]);
  const result = computeVoronoi(equal.rects, equal.total, { clips: 4, expired: () => false });
  assert.ok(result.clips <= 4);
  assert.ok(result.maximumError !== null && result.maximumError < 0.01);
});

test('empty and inconsistent Voronoi inputs keep unknown quality distinct from measured zero', () => {
  const empty = computeVoronoi([], 0, deterministic);
  assert.equal(empty.totalError, null);
  assert.equal(empty.maximumError, null);
  assert.equal(empty.stop, 'empty');
  assert.deepEqual(empty.cells, []);
  const source = createStorageLayout([file(1, 10)]);
  assert.throws(() => computeVoronoi(source.rects, 9, deterministic), /frontier/i);
  assert.throws(
    () => computeVoronoi([...source.rects, ...source.rects], 20, deterministic),
    /frontier/i,
  );
  assert.throws(
    () => computeVoronoi([{ ...source.rects[0], id: VORONOI_GROUP_ID }], 10, deterministic),
    /frontier/i,
  );
  assert.throws(
    () => computeVoronoi([{ ...source.rects[0], value: 0 }], 0, deterministic),
    /frontier/i,
  );
});
