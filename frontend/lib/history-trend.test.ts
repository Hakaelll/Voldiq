import assert from 'node:assert/strict';
import test from 'node:test';
import { historyTrend } from './history-trend.ts';
import { bytes } from './types.ts';

test('snapshot plot preserves real values and spaces points by capture time', () => {
  const captures = [
    { captured: 30, logical: 300 },
    { captured: 0, logical: 100 },
    { captured: 10, logical: 200 },
  ];
  const chart = historyTrend(captures, 376);
  assert.ok(chart);
  assert.deepEqual(
    chart.points.map(({ captured, logical, x, y }) => ({ captured, logical, x, y })),
    [
      { captured: 0, logical: 100, x: 60, y: 180 },
      { captured: 10, logical: 200, x: 160, y: 102 },
      { captured: 30, logical: 300, x: 360, y: 24 },
    ],
  );
  assert.equal(chart.ticks[0].value, 300);
  assert.equal(chart.ticks[3].value, 100);
  assert.equal(captures[0].captured, 30);
});

test('flat sizes and coincident timestamps keep finite points without fabricated captures', () => {
  assert.equal(historyTrend([], 600), null);
  assert.equal(historyTrend([{ captured: 0, logical: 0 }], 600), null);
  for (const logical of [0, 31, 100, 2 ** 60]) {
    const chart = historyTrend(
      [
        { captured: 1, logical },
        { captured: 1, logical },
      ],
      376,
    );
    assert.ok(chart);
    assert.deepEqual(
      chart.points.map((point) => point.x),
      [210, 210],
    );
    assert.equal(chart.points.filter((point) => point.labelled).length, 1);
    for (const tick of chart.ticks) {
      assert.ok(Number.isFinite(tick.y) && tick.y >= 24 && tick.y <= 180);
    }
    for (const point of chart.points) {
      assert.equal(point.logical, logical);
      assert.ok(Number.isFinite(point.y) && point.y >= 24 && point.y <= 180);
    }
  }
});

test('dense history plots every capture while limiting visible labels to five', () => {
  const chart = historyTrend(
    Array.from({ length: 20 }, (_, i) => ({ captured: i, logical: i })),
    600,
  );
  assert.ok(chart);
  assert.equal(chart.points.length, 20);
  assert.equal(chart.points.filter((point) => point.labelled).length, 5);
  assert.equal(chart.points[0].labelled, true);
  assert.equal(chart.points[19].labelled, true);
});

test('zero and one-byte history have valid whole-byte axis labels', () => {
  for (const values of [
    [0, 0],
    [0, 1],
  ]) {
    const chart = historyTrend(
      values.map((logical, captured) => ({ logical, captured })),
      400,
    );
    assert.ok(chart);
    assert.deepEqual(
      chart.ticks.map((tick) => bytes(tick.value)),
      ['1 B', '0 B'],
    );
    assert.deepEqual(
      chart.ticks.map((tick) => tick.y),
      [24, 180],
    );
  }
});

test('clustered capture dates retain every point without overlapping interior labels', () => {
  const chart = historyTrend(
    [0, 1, 2, 3, 100].map((captured) => ({ captured, logical: captured })),
    400,
  );
  assert.ok(chart);
  assert.equal(chart.points.length, 5);
  assert.deepEqual(
    chart.points.filter((point) => point.labelled).map((point) => point.captured),
    [0, 100],
  );
});
