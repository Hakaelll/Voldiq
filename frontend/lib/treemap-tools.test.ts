import { test } from 'node:test';
import assert from 'node:assert/strict';
import { appendLassoPoint, selectLasso, lensCamera } from './treemap-tools.ts';
import { createStorageLayout } from './storage-map.ts';
import { screenToWorld, worldToScreen, type Point } from './disk-city.ts';

const region = (id: number, x: number, y: number, width: number, height: number) => ({
  id,
  x,
  y,
  width,
  height,
});
const outline = (x: number, y: number, width: number, height: number): Point[] => [
  { x, y },
  { x: x + width, y },
  { x: x + width, y: y + height },
  { x, y: y + height },
];

test('a concave lasso selects positive intersections and reports synthetic regions separately', () => {
  const loop = [
    { x: 0, y: 0 },
    { x: 80, y: 0 },
    { x: 80, y: 20 },
    { x: 20, y: 20 },
    { x: 20, y: 80 },
    { x: 0, y: 80 },
  ];
  const rects = [
    region(1, 5, 40, 10, 10),
    region(2, 40, 5, 10, 10),
    region(3, 40, 40, 10, 10),
    region(4, 18, 18, 4, 4),
    region(5, 80, 0, 10, 10),
    region(-8, 2, 2, 10, 10),
  ];
  assert.deepEqual(selectLasso(rects, loop), {
    kind: 'selected',
    ids: [1, 2, 4],
    skippedGroups: 1,
  });
  assert.deepEqual(selectLasso(rects, [...loop].reverse()), {
    kind: 'selected',
    ids: [1, 2, 4],
    skippedGroups: 1,
  });
  assert.deepEqual(selectLasso(rects, [...loop, loop[0]]), {
    kind: 'selected',
    ids: [1, 2, 4],
    skippedGroups: 1,
  });
});

test('lasso intersection covers crossing edges, enclosing shapes and border-only contacts', () => {
  assert.deepEqual(
    selectLasso([region(1, 45, 0, 10, 100), region(2, -5, -5, 110, 110)], outline(0, 40, 100, 20)),
    { kind: 'selected', ids: [1, 2], skippedGroups: 0 },
  );
  assert.deepEqual(
    selectLasso(
      [region(1, 10, 10, 2, 2), region(2, 5, 5, 10, 10)],
      [
        { x: 0, y: 0 },
        { x: 20, y: 0 },
        { x: 0, y: 20 },
      ],
    ),
    { kind: 'selected', ids: [2], skippedGroups: 0 },
  );
  assert.deepEqual(selectLasso([region(1, 80, 80, 1, 1)], outline(0, 0, 10, 10)), {
    kind: 'selected',
    ids: [],
    skippedGroups: 0,
  });
});

test('invalid and excessive lasso geometry fails without exposing a partial selection', () => {
  const rects = [region(1, 0, 0, 10, 10)];
  for (const loop of [
    [],
    [
      { x: 0, y: 0 },
      { x: 1, y: 1 },
    ],
    [
      { x: 0, y: 0 },
      { x: 1, y: 1 },
      { x: 2, y: 2 },
    ],
    [
      { x: NaN, y: 0 },
      { x: 1, y: 0 },
      { x: 1, y: 1 },
    ],
    [
      { x: 0, y: 0 },
      { x: 10, y: 10 },
      { x: 10, y: 0 },
      { x: 0, y: 10 },
    ],
    Array.from({ length: 257 }, (_, i) => ({
      x: Math.cos((i / 257) * 2 * Math.PI),
      y: Math.sin((i / 257) * 2 * Math.PI),
    })),
  ]) {
    const result = selectLasso(rects, loop);
    assert.equal(result.kind, 'invalid');
    assert.equal('ids' in result, false);
  }
  assert.equal(
    selectLasso(
      Array.from({ length: 351 }, (_, id) => region(id, 0, 0, 1, 1)),
      outline(0, 0, 2, 2),
    ).kind,
    'invalid',
  );
});

test('clipping a concave loop preserves disconnected intersections and tiny covered regions', () => {
  const loop = [
    { x: 0, y: 0 },
    { x: 60, y: 0 },
    { x: 60, y: 60 },
    { x: 40, y: 60 },
    { x: 40, y: 20 },
    { x: 20, y: 20 },
    { x: 20, y: 60 },
    { x: 0, y: 60 },
  ];
  assert.deepEqual(
    selectLasso(
      [region(1, 0, 40, 60, 10), region(2, 20, 30, 20, 20), region(3, 0, 0, 1e-8, 1e-8)],
      loop,
    ),
    { kind: 'selected', ids: [1, 3], skippedGroups: 0 },
  );
});

test('the displayed lasso trace stays bounded and retains both ends when simplified', () => {
  const first = { x: 0, y: 0 };
  let points: readonly Point[] = [first];
  assert.equal(appendLassoPoint(points, { x: 1, y: 1 }), points);
  for (let i = 1; i <= 10000; i++) {
    points = appendLassoPoint(points, { x: i * 4, y: i % 40 });
    assert.ok(points.length <= 256);
  }
  assert.deepEqual(points[0], first);
  assert.deepEqual(points.at(-1), { x: 40000, y: 0 });
  const ended = appendLassoPoint(points, { x: 40001, y: 1 }, true);
  assert.deepEqual(ended.at(-1), { x: 40001, y: 1 });
  assert.deepEqual(points.at(-1), { x: 40000, y: 0 });
});

test('pan and zoom preserve lasso identities through the real screen transform', () => {
  const camera = { x: -120, y: 73, scale: 3.5 };
  const loop = outline(10, 15, 8, 9);
  const restored = loop.map((point) => screenToWorld(worldToScreen(point, camera), camera));
  assert.deepEqual(selectLasso([region(7, 12, 17, 1, 1), region(8, 100, 100, 1, 1)], restored), {
    kind: 'selected',
    ids: [7],
    skippedGroups: 0,
  });
});

test('the geometry lens doubles scale and maps its source point to the local lens center', () => {
  const camera = { x: -30, y: 40, scale: 2 };
  const magnified = lensCamera(camera, { x: 170, y: 140 }, { x: 80, y: 80 });
  assert.deepEqual(magnified, { x: -320, y: -120, scale: 4 });
  assert.deepEqual(worldToScreen({ x: 100, y: 50 }, magnified), { x: 80, y: 80 });
  assert.deepEqual(worldToScreen({ x: 110, y: 60 }, magnified), { x: 120, y: 120 });
  assert.deepEqual(camera, { x: -30, y: 40, scale: 2 });
});

test('lasso selection follows the actual frontier without substituting structural parents for groups', () => {
  const nodes = [
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
  const wholeMap = outline(-1, -1, 1002, 702);
  const shallow = selectLasso(createStorageLayout(nodes, 1).rects, wholeMap);
  const deep = selectLasso(createStorageLayout(nodes, 6).rects, wholeMap);
  assert.equal(shallow.kind, 'selected');
  assert.equal(deep.kind, 'selected');
  if (shallow.kind !== 'selected' || deep.kind !== 'selected') return;
  assert.deepEqual(
    shallow.ids.slice().sort((a, b) => a - b),
    [2, 5],
  );
  assert.deepEqual(
    deep.ids.slice().sort((a, b) => a - b),
    [3, 4, 5],
  );
  assert.equal(deep.skippedGroups, 1);
});
