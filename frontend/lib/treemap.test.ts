import { test } from 'node:test';
import assert from 'node:assert/strict';
import { layout, nestedLayout, mapColor, color, rankColor } from './treemap.ts';

test('ranked map families retain paired ink and share their parent paint', () => {
  const paint = rankColor(0);
  assert.deepEqual(paint, { fill: '#075bdd', ink: '#ffffff' });
  assert.deepEqual(rankColor(99), rankColor(5));
  assert.deepEqual(rankColor(-1), paint);
  const rects = nestedLayout(
    [
      {
        id: 1,
        name: 'root',
        value: 100,
        directory: true,
        paint,
        children: [{ id: 2, name: 'child', value: 60, directory: false }],
      },
    ],
    400,
    300,
  );
  assert.equal(rects.length, 3);
  assert.ok(rects.every((rect) => rect.paint === paint));
});

test('nested map preserves child ratios and measured remainder inside its parent', () => {
  const rects = nestedLayout(
    [
      {
        id: 1,
        name: 'parent',
        value: 100,
        directory: true,
        children: [
          { id: 2, name: 'a', value: 60, directory: true },
          { id: 3, name: 'b', value: 20, directory: false },
        ],
      },
    ],
    400,
    300,
  );
  assert.equal(rects.length, 4);
  const [parent, ...children] = rects;
  assert.deepEqual(
    children.map((item) => item.value),
    [60, 20, 20],
  );
  assert.equal(children[2].name, 'Other');
  const area = (400 - 12) * (300 - 58);
  for (const child of children) {
    assert.ok(Math.abs((child.width * child.height) / area - child.value / 100) < 1e-8);
    assert.ok(child.x >= parent.x + 6 && child.y >= parent.y + 52);
    assert.ok(child.x + child.width <= parent.x + parent.width - 6 + 1e-8);
    assert.ok(child.y + child.height <= parent.y + parent.height - 6 + 1e-8);
  }
  assert.equal(nestedLayout([{ ...parent, value: 10 }], 400, 300).length, 1);
  assert.equal(nestedLayout([parent], 100, 100).length, 1);
});

test('all map palette fills have readable paired ink and consistent legend colors', () => {
  const luminance = (hex: string) => {
    const channel = (offset: number) => {
      const value = parseInt(hex.slice(offset, offset + 2), 16) / 255;
      return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
    };
    return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
  };
  const seen = new Set<string>();
  for (const name of ['a', 'b', 'c', 'd', 'e', 'f']) {
    const paint = mapColor(name);
    seen.add(paint.fill);
    assert.equal(color(name), paint.fill);
    const fill = luminance(paint.fill),
      ink = luminance(paint.ink);
    const contrast = (Math.max(fill, ink) + 0.05) / (Math.min(fill, ink) + 0.05);
    assert.ok(contrast >= 4.5, `${paint.fill}/${paint.ink}: ${contrast}`);
  }
  assert.equal(seen.size, 6);
});

test('rectangle areas preserve measured ratios and stay inside the viewport', () => {
  const nodes = Array.from({ length: 100 }, (_, id) => ({
    id,
    name: `${id}`,
    value: id + 1,
    directory: false,
  }));
  const rects = layout(nodes, 840, 460);
  const total = nodes.reduce((s, n) => s + n.value, 0);
  assert.equal(rects.length, 100);
  for (const rect of rects) {
    assert.ok(Math.abs((rect.width * rect.height) / (840 * 460) - rect.value / total) < 1e-10);
    assert.ok(
      rect.x >= -1e-8 &&
        rect.y >= -1e-8 &&
        rect.x + rect.width <= 840 + 1e-8 &&
        rect.y + rect.height <= 460 + 1e-8,
    );
  }
  for (let i = 0; i < rects.length; i++)
    for (let j = i + 1; j < rects.length; j++) {
      const a = rects[i],
        b = rects[j];
      if (a && b)
        assert.ok(
          Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x) < 1e-8 ||
            Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y) < 1e-8,
        );
    }
});
test('empty, invalid and zero-sized inputs produce no rectangles', () => {
  assert.deepEqual(layout([], 100, 100), []);
  assert.deepEqual(layout([{ id: 0, name: 'empty', value: 0, directory: true }], 100, 100), []);
  assert.deepEqual(layout([{ id: 0, name: 'invalid', value: NaN, directory: true }], 100, 100), []);
});
