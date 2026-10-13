import assert from 'node:assert/strict';
import test from 'node:test';
import {
  virtualWindow,
  rowTop,
  scrollToRow,
  keyboardRow,
  rowHeight,
  wheelScroll,
} from './virtual-list.ts';

test('five-million-row indices reach their last row within a bounded physical height', () => {
  const total = 5_000_000;
  const first = virtualWindow(total, 420, 0);
  assert.equal(first.height, 8_000_000);
  assert.equal(first.offset, 0);
  const scroll = scrollToRow(first, total - 1);
  const last = virtualWindow(total, 420, scroll);
  assert.equal(last.physicalTop, last.physicalRange);
  assert.equal(last.logicalTop, total * rowHeight - 384);
  assert.ok(last.offset < total && last.offset >= total - 20);
  assert.equal(rowTop(last, total - 1) + rowHeight, last.height);
  assert.equal(scrollToRow(last, 0), 0);
});

test('ordinary lists retain native row spacing and large lists keep visible rows uncompressed', () => {
  for (const total of [0, 1, 10, 1000, 1_000_000, 5_000_000]) {
    const first = virtualWindow(total, 420, 0);
    for (const fraction of [0, 0.1, 0.5, 0.99, 1]) {
      const window = virtualWindow(total, 420, first.physicalRange * fraction);
      assert.ok(window.offset >= 0 && window.offset <= total);
      const row = Math.floor(window.logicalTop / rowHeight);
      assert.ok(Math.abs(rowTop(window, row + 1) - rowTop(window, row) - rowHeight) < 0.000001);
      if (total <= 1000) assert.equal(window.logicalTop, window.physicalTop);
      if (total > 10) {
        const screenTop = rowTop(window, row) - window.physicalTop;
        assert.ok(screenTop <= 0 && screenTop > -rowHeight);
      }
    }
  }
  assert.equal(virtualWindow(1000, 420, -100).physicalTop, 0);
  assert.equal(virtualWindow(1000, 420, Number.MAX_SAFE_INTEGER).physicalTop, 35616);
});

test('keyboard movement clamps at endpoints and moves by visible row counts', () => {
  assert.equal(keyboardRow('ArrowUp', 0, 1_000_000, 384), 0);
  assert.equal(keyboardRow('ArrowDown', 999999, 1_000_000, 384), 999999);
  assert.equal(keyboardRow('End', 0, 5_000_000, 384), 4999999);
  assert.equal(keyboardRow('Home', 4999999, 5_000_000, 384), 0);
  assert.equal(keyboardRow('PageDown', 100, 1000, 384), 110);
  assert.equal(keyboardRow('PageUp', 5, 1000, 384), 0);
  assert.equal(keyboardRow('ArrowDown', 0, 0, 384), null);
  assert.equal(keyboardRow('Enter', 10, 1000, 384), null);
});

test('wheel movement preserves logical pixels, lines and pages in a compressed range', () => {
  const wheel = { deltaX: 0, ctrlKey: false, shiftKey: false };
  const first = virtualWindow(5_000_000, 420, 0);
  const middle = virtualWindow(5_000_000, 420, first.physicalRange / 2);
  for (const [deltaMode, expected] of [
    [0, 1],
    [1, 36],
    [2, 384],
  ]) {
    for (const deltaY of [-1, 1, 40]) {
      const physical = wheelScroll(middle, { ...wheel, deltaMode, deltaY });
      assert.ok(physical !== null);
      const next = virtualWindow(5_000_000, 420, physical);
      assert.ok(Math.abs(next.logicalTop - middle.logicalTop - deltaY * expected) < 0.000001);
    }
  }
  assert.equal(wheelScroll(first, { ...wheel, deltaMode: 0, deltaY: -100 }), 0);
  const last = virtualWindow(5_000_000, 420, first.physicalRange);
  assert.equal(wheelScroll(last, { ...wheel, deltaMode: 0, deltaY: 100 }), last.physicalRange);
});

test('diagonal, horizontal and modified gestures retain native scrolling and zoom behavior', () => {
  const window = virtualWindow(5_000_000, 420, 1000);
  const wheel = { deltaX: 0, deltaY: 40, deltaMode: 0, ctrlKey: false, shiftKey: false };
  for (const changes of [
    { deltaX: 15 },
    { deltaX: 15, deltaY: 0 },
    { ctrlKey: true },
    { shiftKey: true },
    { deltaY: 0 },
  ]) {
    assert.equal(wheelScroll(window, { ...wheel, ...changes }), null);
  }
  assert.equal(wheelScroll(virtualWindow(1000, 420, 100), wheel), null);
});
