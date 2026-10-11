import { test } from 'node:test';
import assert from 'node:assert/strict';
import { layout } from './treemap.ts';

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
