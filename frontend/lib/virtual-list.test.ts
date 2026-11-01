import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  virtualWindow,
  rowTop,
  scrollToRow,
  keyboardRow,
  wheelScroll,
  gridDimensions,
} from './virtual-list.ts';

test('a multi-column window pages complete rows and places every item in its expected row', () => {
  const window = virtualWindow(1000, 400, 600, {
    columns: 4,
    rowHeight: 150,
    headerHeight: 0,
    overscan: 2,
  });
  assert.equal(window.height, 37500);
  assert.equal(window.logicalTop, 600);
  assert.equal(window.offset, 8);
  assert.equal(window.limit, 32);
  assert.equal(rowTop(window, 16), 600);
  assert.equal(rowTop(window, 19), 600);
  assert.equal(rowTop(window, 20), 750);
  assert.equal(scrollToRow(window, 0), 0);
  assert.equal(scrollToRow(window, 999), 37100);
});

test('grid dimensions cap requested items while covering narrow and tall viewports', () => {
  for (const width of [0, 120, 352, 700, 1408, 10000]) {
    for (const height of [0, 176, 900, 5000, 100000]) {
      const geometry = gridDimensions(width, height);
      assert.ok(geometry.columns >= 1 && geometry.columns <= 8);
      const window = virtualWindow(1_000_003, height, 1000000, geometry);
      assert.equal(window.offset % geometry.columns, 0);
      assert.ok(window.limit > 0 && window.limit <= 500);
      const first = Math.floor(window.logicalTop / geometry.rowHeight) * geometry.columns;
      const after =
        Math.ceil((window.logicalTop + window.visibleHeight) / geometry.rowHeight) *
        geometry.columns;
      assert.ok(window.offset <= first && window.offset + window.limit >= after);
    }
  }
  assert.equal(gridDimensions(100, 900).columns, 1);
  assert.equal(gridDimensions(352, 900).columns, 2);
  assert.equal(gridDimensions(2000, 900).columns, 8);
});

test('changing columns or table representation can preserve the first visible item', () => {
  const before = virtualWindow(10000, 640, 3520, {
    columns: 4,
    rowHeight: 176,
    headerHeight: 0,
    overscan: 2,
  });
  assert.equal(Math.floor(before.logicalTop / before.rowHeight) * before.columns, 80);
  const table = virtualWindow(10000, 640, 3520);
  const changed = virtualWindow(10000, 640, scrollToRow(table, 80, 'start'));
  assert.equal(changed.logicalTop, 2880);
  assert.equal(rowTop(changed, 80), changed.physicalTop);
  const empty = virtualWindow(0, 640, 100, gridDimensions(700, 640));
  assert.equal(empty.height, 0);
  assert.equal(empty.logicalTop, 0);
  assert.equal(empty.offset, 0);
  assert.equal(scrollToRow(empty, 0), 0);
});

test('compressed scrolling reaches the last grid item and wheel input moves by logical pixels', () => {
  const options = { columns: 8, rowHeight: 150, headerHeight: 0, overscan: 2 };
  const bottom = virtualWindow(1_000_003, 600, Number.MAX_SAFE_INTEGER, options);
  assert.equal(bottom.height, 8_000_000);
  assert.ok(rowTop(bottom, 1_000_002) + 150 <= bottom.physicalTop + 600 + 1e-6);
  const first = virtualWindow(1_000_003, 600, 0, options);
  const top = wheelScroll(first, {
    deltaY: 150,
    deltaX: 0,
    deltaMode: 0,
    ctrlKey: false,
    shiftKey: false,
  });
  assert.ok(top !== null);
  const moved = virtualWindow(1_000_003, 600, top, options);
  assert.ok(Math.abs(moved.logicalTop - 150) < 1e-6);
  assert.equal(scrollToRow(first, 1_000_002), bottom.physicalRange);
});

test('grid keyboard movement uses columns, full pages and bounded item indices', () => {
  assert.equal(keyboardRow('ArrowDown', 1, 19, 450, 4, 150), 5);
  assert.equal(keyboardRow('ArrowUp', 5, 19, 450, 4, 150), 1);
  assert.equal(keyboardRow('ArrowRight', 5, 19, 450, 4, 150), 6);
  assert.equal(keyboardRow('ArrowLeft', 5, 19, 450, 4, 150), 4);
  assert.equal(keyboardRow('PageDown', 1, 19, 450, 4, 150), 13);
  assert.equal(keyboardRow('PageUp', 13, 19, 450, 4, 150), 1);
  assert.equal(keyboardRow('End', 1, 19, 450, 4, 150), 18);
  assert.equal(keyboardRow('ArrowDown', 17, 19, 450, 4, 150), 18);
  assert.equal(keyboardRow('Home', 18, 19, 450, 4, 150), 0);
  assert.equal(keyboardRow('ArrowRight', 3, 10, 400), null);
  assert.equal(keyboardRow('End', 0, 0, 400, 4, 150), null);
});
