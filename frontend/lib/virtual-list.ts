export const rowHeight = 36;
const maxHeight = 8_000_000;

export function virtualWindow(total: number, viewportHeight: number, scrollTop: number) {
  const visibleHeight = Math.max(rowHeight, viewportHeight - rowHeight);
  const logicalHeight = total * rowHeight;
  const height = Math.min(maxHeight, logicalHeight);
  const physicalRange = Math.max(0, height - visibleHeight);
  const logicalRange = Math.max(0, logicalHeight - visibleHeight);
  const physicalTop = Math.min(physicalRange, Math.max(0, scrollTop));
  const logicalTop = physicalRange ? (physicalTop / physicalRange) * logicalRange : 0;
  return {
    height,
    physicalRange,
    logicalRange,
    physicalTop,
    logicalTop,
    visibleHeight,
    offset: Math.max(0, Math.floor(logicalTop / rowHeight) - 8),
  };
}

export function rowTop(window: ReturnType<typeof virtualWindow>, index: number): number {
  return window.physicalTop + index * rowHeight - window.logicalTop;
}

export function wheelScroll(
  window: ReturnType<typeof virtualWindow>,
  event: Pick<WheelEvent, 'deltaY' | 'deltaX' | 'deltaMode' | 'ctrlKey' | 'shiftKey'>,
): number | null {
  if (
    event.ctrlKey ||
    event.shiftKey ||
    event.deltaX !== 0 ||
    event.deltaY === 0 ||
    window.logicalRange <= window.physicalRange
  )
    return null;
  const delta =
    event.deltaY *
    (event.deltaMode === 1 ? rowHeight : event.deltaMode === 2 ? window.visibleHeight : 1);
  return window.logicalRange
    ? Math.min(
        window.physicalRange,
        Math.max(0, window.physicalTop + (delta * window.physicalRange) / window.logicalRange),
      )
    : 0;
}

export function scrollToRow(window: ReturnType<typeof virtualWindow>, index: number): number {
  const start = index * rowHeight;
  const end = start + rowHeight;
  const top =
    start < window.logicalTop
      ? start
      : end > window.logicalTop + window.visibleHeight
        ? end - window.visibleHeight
        : window.logicalTop;
  return window.logicalRange
    ? (Math.min(window.logicalRange, Math.max(0, top)) / window.logicalRange) * window.physicalRange
    : 0;
}

export function keyboardRow(
  key: string,
  current: number,
  total: number,
  visibleHeight: number,
): number | null {
  if (total === 0) return null;
  const page = Math.max(1, Math.floor(visibleHeight / rowHeight));
  let next: number;
  switch (key) {
    case 'ArrowDown':
      next = current + 1;
      break;
    case 'ArrowUp':
      next = current - 1;
      break;
    case 'PageDown':
      next = current + page;
      break;
    case 'PageUp':
      next = current - page;
      break;
    case 'Home':
      next = 0;
      break;
    case 'End':
      next = total - 1;
      break;
    default:
      return null;
  }
  return Math.min(total - 1, Math.max(0, next));
}
