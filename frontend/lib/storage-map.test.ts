import { test } from 'node:test';
import assert from 'node:assert/strict';
import { categoryLabel, categoryPaint, createStorageLayout, heightDescription, metricHeight, metricMaximum, metricValue, visualPaint } from './storage-map.ts';
import type { WeightedNode } from './treemap.ts';

const file = (id: number, value: number, name = `${id}.txt`): WeightedNode => ({ id, value, name, directory: false });
const near = (actual: number, expected: number): void => {
  assert.ok(Math.abs(actual - expected) <= Math.max(1e-8, expected * 1e-10), `${actual} != ${expected}`);
};

test('frontier conserves bytes and proportional area while boundaries contain children', () => {
  const nodes: WeightedNode[] = [
    { id: 2, name: 'Photos', value: 800, directory: true, files: 8, allocationKnown: true,
      children: [file(3, 400), file(4, 200)] },
    file(5, 200),
  ];
  const result = createStorageLayout(nodes);
  assert.equal(result.total, 1000);
  assert.equal(result.rects.some((rect) => rect.id === 2), false);
  assert.equal(result.boundaries[0]?.id, 2);
  assert.equal(result.rects.find((rect) => rect.id === -4)?.value, 200);
  assert.equal(result.rects.reduce((sum, rect) => sum + rect.value, 0), 1000);
  near(result.rects.reduce((sum, rect) => sum + rect.width * rect.height, 0), 700000);
  for (const rect of result.rects) {
    near(rect.width * rect.height / 700000, rect.value / 1000);
    assert.ok(rect.x >= 0 && rect.y >= 0 && rect.x + rect.width <= 1000 + 1e-8 && rect.y + rect.height <= 700 + 1e-8);
    for (const other of result.rects) {
      if (rect.id === other.id) continue;
      const overlap = Math.max(0, Math.min(rect.x + rect.width, other.x + other.width) - Math.max(rect.x, other.x)) *
        Math.max(0, Math.min(rect.y + rect.height, other.y + other.height) - Math.max(rect.y, other.y));
      near(overlap, 0);
    }
  }
  assert.deepEqual(createStorageLayout(nodes.slice().reverse()), result);
});

test('dominant entries, thin slivers, and a root remainder stay deterministic', () => {
  const nodes = [file(1, 1e12), ...Array.from({ length: 249 }, (_, index) => file(index + 2, 1)),
    { id: -1, name: 'Other contents', value: 50, directory: true }];
  const first = createStorageLayout(nodes);
  const second = createStorageLayout(nodes.slice().reverse());
  assert.equal(first.revision, second.revision);
  assert.equal(first.rects.length, 251);
  assert.ok(first.rects.every((rect) => Number.isFinite(rect.width) && Number.isFinite(rect.height) && rect.width > 0 && rect.height > 0));
  near(first.rects.reduce((sum, rect) => sum + rect.value, 0), first.total);
  near(first.rects.reduce((sum, rect) => sum + rect.width * rect.height, 0), 700000);
  assert.equal(first.rects.find((rect) => rect.id === -1)?.name, 'Other contents');
});

test('empty data, zero bytes, incomplete and invalid branches have honest aggregates', () => {
  assert.deepEqual(createStorageLayout([]).rects, []);
  const result = createStorageLayout([{ ...file(1, 0), allocationKnown: true }, file(2, -3), file(3, NaN),
    { id: 4, name: 'Partial', value: 100, directory: true, incomplete: true, children: [file(5, 25)] }]);
  assert.equal(result.zeroBytes, 1);
  assert.equal(createStorageLayout([file(1, 0)]).zeroBytes, 0);
  assert.equal(result.total, 100);
  assert.equal(result.rects.find((rect) => rect.id === -6)?.name, 'Unscanned or unlisted contents');
  const invalid = createStorageLayout([{ id: 10, name: 'Invalid', value: 5, directory: true, children: [file(11, 6)] }]);
  assert.deepEqual(invalid.rects.map((rect) => rect.id), [10]);
  assert.equal(invalid.boundaries.length, 0);
});

test('frontier expansion has a bounded rectangle count', () => {
  const nodes = Array.from({ length: 250 }, (_, index): WeightedNode => ({
    id: index + 1, name: `${index}`, value: index ? 1 : 100000, directory: true,
    children: Array.from({ length: 200 }, (_, child) => file(1000 + index * 200 + child, 1)),
  }));
  const result = createStorageLayout(nodes);
  assert.ok(result.rects.length <= 350);
  near(result.rects.reduce((sum, rect) => sum + rect.value, 0), result.total);
});

test('directory age uses descendant coverage and unknown times stay unknown', () => {
  const now = 2_000_000_000;
  const ownOnly = createStorageLayout([{ id: 1, name: 'Directory', value: 10, directory: true, modified: now, files: 20 }]).rects[0];
  assert.ok(ownOnly);
  assert.equal(ownOnly.modified, null);
  assert.equal(metricValue(ownOnly, 'modified', now), null);
  assert.equal(metricHeight(ownOnly, 'modified', now, 100), 5);
  assert.match(heightDescription(ownOnly, 'modified', now), /unknown/);
  assert.equal(ownOnly.allocation_known, false);
  const descendants = createStorageLayout([{ id: 2, name: 'Known directory', value: 10, directory: true,
    modified: now, latestModified: now - 86400, modifiedKnown: 2, modifiedTotal: 20, files: 20 }]).rects;
  const known = descendants[0];
  assert.ok(known);
  assert.equal(metricValue(known, 'modified', now), 86400);
  assert.equal(metricMaximum(descendants, 'modified', now), 86400);
  near(metricHeight(known, 'modified', now, 86400), 5 + 125 * Math.log1p(1) / Math.log1p(3652.5));
  assert.match(heightDescription(known, 'modified', now), /2\/20 known/);
  assert.equal(metricHeight(known, 'files', now, 20), 130);
  assert.equal(metricHeight(known, 'depth', now, 0), 5);
  assert.deepEqual(visualPaint(ownOnly, 'age', 10, now), categoryPaint({ ...ownOnly, category: 'remainder' }));
});

test('heights use fixed logarithmic age, local logarithmic files, and local linear depth', () => {
  const now = 2_000_000_000;
  const rect = createStorageLayout([file(1, 10)]).rects[0];
  assert.ok(rect);
  near(metricHeight({ ...rect, modified: now - 180 * 86400 }, 'modified', now, 1),
    5 + 125 * Math.log1p(180) / Math.log1p(3652.5));
  assert.equal(metricHeight({ ...rect, modified: now - 180 * 86400 }, 'modified', now, 1),
    metricHeight({ ...rect, modified: now - 180 * 86400 }, 'modified', now, 1e12));
  assert.equal(metricHeight({ ...rect, modified: now - 3652.5 * 86400 }, 'modified', now, 0), 130);
  assert.equal(metricHeight({ ...rect, modified: now - 20000 * 86400 }, 'modified', now, 0), 130);
  assert.equal(metricHeight({ ...rect, modified: now + 1000 }, 'modified', now, 1000), 5);
  near(metricHeight({ ...rect, files: 9 }, 'files', now, 99), 5 + 125 * Math.log1p(9) / Math.log1p(99));
  near(metricHeight({ ...rect, depth: 2 }, 'depth', now, 8), 36.25);
  assert.equal(metricHeight({ ...rect, files: 0 }, 'files', now, 0), 5);
});

test('represented counts are file counts and directory extensions do not classify folders', () => {
  const result = createStorageLayout([
    { id: 1, name: 'empty.mp4', value: 10, directory: true, files: 0 },
    { id: 2, name: 'full', value: 100, directory: true, files: 5, children: [file(3, 40)] },
  ]);
  assert.equal(result.represented, 5);
  assert.equal(result.rects.reduce((sum, rect) => sum + rect.represented, 0), 5);
  assert.equal(result.rects.find((rect) => rect.id === 1)?.aggregated, 0);
  const folder = { id: 5, name: 'recording.mp4', value: 1, directory: true };
  assert.equal(categoryLabel(folder), 'Folder');
  assert.deepEqual(categoryPaint(folder), categoryPaint({ ...folder, name: 'Folder' }));
  assert.equal(categoryLabel(file(6, 1, 'recording.mp4')), 'Video');
});

function luminance(hex: string): number {
  const values = [1, 3, 5].map((offset) => parseInt(hex.slice(offset, offset + 2), 16) / 255)
    .map((value) => value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4);
  return (values[0] ?? 0) * 0.2126 + (values[1] ?? 0) * 0.7152 + (values[2] ?? 0) * 0.0722;
}

test('all category and metric paints have readable text contrast', () => {
  const nodes = ['photo.png', 'movie.mp4', 'music.mp3', 'code.ts', 'archive.zip', 'document.pdf', 'unknown.bin']
    .map((name, index) => file(index + 1, 10, name));
  nodes.push({ id: -1, name: 'Other contents', value: 10, directory: true },
    { id: 20, name: 'Folder', value: 10, directory: true });
  const rects = createStorageLayout(nodes).rects;
  for (const rect of rects) {
    const colors = [categoryPaint(rect), ...[0, 40, 200, 400, 4000].flatMap((days) => [
      visualPaint({ ...rect, modified: 2_000_000_000 - days * 86400 }, 'age', 10, 2_000_000_000),
      visualPaint(rect, 'size', 10 * (days + 1), 2_000_000_000),
    ])];
    for (const paint of colors) {
      const light = Math.max(luminance(paint.fill), luminance(paint.ink));
      const dark = Math.min(luminance(paint.fill), luminance(paint.ink));
      assert.ok((light + 0.05) / (dark + 0.05) >= 4.5, `${paint.fill} / ${paint.ink}`);
    }
  }
});

test('size and age colors darken sequentially and age ignores local byte maximum', () => {
  const rect = createStorageLayout([file(1, 1)]).rects[0];
  assert.ok(rect);
  const now = 2_000_000_000;
  const sizes = [0, 1, 10, 100, 1000, 10000].map((value) =>
    luminance(visualPaint({ ...rect, value }, 'size', 10000, now).fill));
  const ages = [0, 30, 180, 365, 3652].map((days) => {
    const dated = { ...rect, modified: now - days * 86400 };
    assert.deepEqual(visualPaint(dated, 'age', 1, now), visualPaint(dated, 'age', 1e12, now));
    return luminance(visualPaint(dated, 'age', 1, now).fill);
  });
  for (const sequence of [sizes, ages]) {
    assert.ok(sequence[0] !== sequence.at(-1));
    sequence.forEach((light, index) => { if (index) assert.ok(light <= (sequence[index - 1] ?? 0)); });
  }
  assert.deepEqual(visualPaint({ ...rect, value: 9 }, 'size', 9999, now),
    visualPaint({ ...rect, value: 99 }, 'size', 99_999_999, now));
});
