import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createStorageLayout } from './storage-map.ts';
import { radialLayout, radialHit, shapeArea, shapeAnchor } from './radial-map.ts';
import type { WeightedNode } from './treemap.ts';

const file = (id: number, value: number): WeightedNode => ({
  id,
  value,
  name: `${id}.bin`,
  directory: false,
  allocationKnown: true,
});
const near = (a: number, b: number): void =>
  assert.ok(Math.abs(a - b) <= Math.max(1e-9, Math.abs(b) * 1e-9), `${a} != ${b}`);
const tree: WeightedNode[] = [
  {
    id: 1,
    name: 'projects',
    directory: true,
    value: 900,
    files: 9,
    allocationKnown: true,
    children: [
      {
        id: 2,
        name: 'build',
        directory: true,
        value: 700,
        files: 7,
        allocationKnown: true,
        children: [file(3, 400), file(4, 200)],
      },
      file(5, 200),
    ],
  },
  file(6, 100),
];

test('nested circles do not overlap and their structural ancestors enclose them without rescaling data', () => {
  const source = createStorageLayout(tree);
  const result = radialLayout(source, 'circles');
  for (const child of result.shapes) {
    assert.equal(child.kind, 'circle');
    if (child.kind !== 'circle') throw new Error('Expected circles');
    let parent = source.parents.get(child.node.id);
    while (parent !== null && parent !== undefined) {
      const container = result.shapes.find((shape) => shape.node.id === parent);
      assert.ok(container?.kind === 'circle' && container.structural);
      assert.ok(
        Math.hypot(child.x - container.x, child.y - container.y) + child.radius <=
          container.radius + 1e-8,
      );
      parent = source.parents.get(parent);
    }
    for (const sibling of result.shapes) {
      if (
        sibling.kind !== 'circle' ||
        sibling.node.id === child.node.id ||
        source.parents.get(sibling.node.id) !== source.parents.get(child.node.id)
      )
        continue;
      assert.ok(
        Math.hypot(child.x - sibling.x, child.y - sibling.y) >=
          child.radius + sibling.radius - 1e-8,
      );
    }
    assert.ok(
      child.x - child.radius >= 0 &&
        child.x + child.radius <= 1000 &&
        child.y - child.radius >= 0 &&
        child.y + child.radius <= 700,
    );
  }
});

test('sunburst child angles remain inside ancestors and equal-area rings do not inflate shallow entries', () => {
  const source = createStorageLayout(tree);
  const result = radialLayout(source, 'sunburst');
  for (const shape of result.shapes) {
    if (shape.kind !== 'sector') throw new Error('Expected sectors');
    near(shape.end - shape.start, (2 * Math.PI * shape.node.value) / source.total);
    const parent = source.parents.get(shape.node.id);
    if (parent !== null && parent !== undefined) {
      const container = result.shapes.find((item) => item.node.id === parent);
      assert.ok(container?.kind === 'sector');
      assert.ok(shape.start >= container.start - 1e-12 && shape.end <= container.end + 1e-12);
      near(shape.inner, container.outer);
    }
  }
  assert.equal(radialHit(result, { x: 500, y: 350 }), null);
});

test('radial geometry stays bounded with dominant entries and tiny values', () => {
  const items = [file(1, 1e12), ...Array.from({ length: 249 }, (_, i) => file(i + 2, 1))];
  const source = createStorageLayout(items);
  for (const view of ['sunburst', 'circles'] as const) {
    const result = radialLayout(source, view);
    assert.equal(result.shapes.length, 250);
    const data = result.shapes.filter((shape) => !shape.structural);
    const scale = shapeArea(data[0]) / data[0].node.value;
    for (const shape of data) {
      assert.ok(Number.isFinite(shapeArea(shape)));
      assert.ok(Math.abs(shapeArea(shape) / shape.node.value / scale - 1) < 0.001);
      assert.equal(radialHit(result, shapeAnchor(shape))?.id, shape.node.id);
    }
  }
});

test('six-level navigation picks measured leaves and structural parents without counting parents twice', () => {
  let branch: WeightedNode = file(7, 1000);
  for (let id = 6; id > 0; id--)
    branch = { ...file(id, 1000), directory: true, children: [branch] };
  const source = createStorageLayout([branch]);
  assert.equal(source.boundaries.length, 5);
  assert.deepEqual(
    source.rects.map((node) => node.id),
    [6],
  );
  for (const view of ['sunburst', 'circles'] as const) {
    const result = radialLayout(source, view);
    assert.equal(result.shapes.length, 6);
    const data = result.shapes.filter((shape) => !shape.structural);
    assert.deepEqual(
      data.map((shape) => shape.node.id),
      [6],
    );
    assert.equal(radialHit(result, shapeAnchor(data[0]))?.id, 6);
    const parent = result.shapes.find((shape) => shape.node.id === 1);
    assert.ok(parent?.structural);
    const point =
      parent.kind === 'circle'
        ? { x: parent.x + parent.radius * 0.99, y: parent.y }
        : shapeAnchor(parent);
    assert.equal(radialHit(result, point)?.id, 1);
  }
});

test('radial views use the requested common depth without changing byte totals or area meaning', () => {
  for (const depth of [1, 2, 3, 4, 5, 6] as const) {
    const source = createStorageLayout(tree, depth);
    for (const view of ['sunburst', 'circles'] as const) {
      const result = radialLayout(source, view);
      assert.ok(result.dataFraction > 0 && result.dataFraction <= 1);
      assert.deepEqual(result, radialLayout(source, view));
      const measured = result.shapes.filter((shape) => !shape.structural);
      assert.deepEqual(
        measured.map((shape) => shape.node.id).sort((a, b) => a - b),
        source.rects.map((node) => node.id).sort((a, b) => a - b),
      );
      assert.equal(
        measured.reduce((sum, shape) => sum + shape.node.value, 0),
        1000,
      );
      const scale = shapeArea(measured[0]) / measured[0].node.value;
      for (const shape of measured) {
        assert.ok(shape.node.depth <= depth);
        near(shapeArea(shape) / shape.node.value, scale);
        assert.equal(radialHit(result, shapeAnchor(shape))?.id, shape.node.id);
      }
    }
  }
});

test('empty, zero and unknown byte entries do not invent measurable area; inconsistent hierarchy fails explicitly', () => {
  for (const view of ['sunburst', 'circles'] as const) {
    assert.deepEqual(radialLayout(createStorageLayout([]), view).shapes, []);
    assert.deepEqual(
      radialLayout(
        createStorageLayout([file(1, 0), { ...file(2, 0), allocationKnown: false }]),
        view,
      ).shapes,
      [],
    );
    const source = createStorageLayout(tree);
    assert.equal(radialHit(radialLayout(source, view), { x: 0, y: 0 }), null);
    source.parents.set(source.rects[0].id, source.rects[0].id);
    assert.throws(() => radialLayout(source, view), /hierarchy/i);
  }
});
