export const rowHeight = 36;
const maxHeight = 8_000_000;
export interface WindowGeometry {
  columns: number;
  rowHeight: number;
  headerHeight: number;
  overscan: number;
}
export function gridDimensions(width: number, height: number): WindowGeometry {
  const columns = Math.max(1, Math.min(8, Math.floor(width / 176)));
  return {
    columns,
    rowHeight: Math.max(176, Math.ceil(height / (Math.floor(500 / columns) - 5))),
    headerHeight: 0,
    overscan: 2,
  };
}

export function virtualWindow(
  total: number,
  viewportHeight: number,
  scrollTop: number,
  geometry?: WindowGeometry,
) {
  const columns = geometry?.columns ?? 1;
  const extent = geometry?.rowHeight ?? rowHeight;
  const overscan = geometry?.overscan ?? 8;
  const visibleHeight = Math.max(extent, viewportHeight - (geometry?.headerHeight ?? rowHeight));
  const logicalHeight = Math.ceil(total / columns) * extent;
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
    columns,
    rowHeight: extent,
    offset: Math.max(0, Math.floor(logicalTop / extent) - overscan) * columns,
    limit: Math.min(500, (Math.ceil(visibleHeight / extent) + 1 + 2 * overscan) * columns),
  };
}

export function rowTop(window: ReturnType<typeof virtualWindow>, index: number): number {
  return (
    window.physicalTop + Math.floor(index / window.columns) * window.rowHeight - window.logicalTop
  );
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
    (event.deltaMode === 1 ? window.rowHeight : event.deltaMode === 2 ? window.visibleHeight : 1);
  return window.logicalRange
    ? Math.min(
        window.physicalRange,
        Math.max(0, window.physicalTop + (delta * window.physicalRange) / window.logicalRange),
      )
    : 0;
}

export function scrollToRow(
  window: ReturnType<typeof virtualWindow>,
  index: number,
  align: 'nearest' | 'start' = 'nearest',
): number {
  const start = Math.floor(index / window.columns) * window.rowHeight;
  const end = start + window.rowHeight;
  const top =
    align === 'start' || start < window.logicalTop
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
  columns = 1,
  extent = rowHeight,
): number | null {
  if (total === 0) return null;
  const page = Math.max(1, Math.floor(visibleHeight / extent)) * columns;
  let next: number;
  switch (key) {
    case 'ArrowDown':
      next = current + columns;
      break;
    case 'ArrowUp':
      next = current - columns;
      break;
    case 'ArrowLeft':
    case 'ArrowRight':
      if (columns === 1) return null;
      next = current + (key === 'ArrowRight' ? 1 : -1);
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
