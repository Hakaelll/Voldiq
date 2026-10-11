export interface WeightedNode {
  id: number;
  name: string;
  value: number;
  directory: boolean;
}
export interface Rect extends WeightedNode {
  x: number;
  y: number;
  width: number;
  height: number;
}

export function layout(nodes: WeightedNode[], width: number, height: number): Rect[] {
  if (width <= 0 || height <= 0) return [];
  const items = nodes
    .filter((n) => Number.isFinite(n.value) && n.value > 0)
    .sort((a, b) => b.value - a.value);
  const total = items.reduce((sum, n) => sum + n.value, 0);
  if (total === 0) return [];
  const scale = (width * height) / total;
  let area = { x: 0, y: 0, width, height };
  const output: Rect[] = [];
  const worst = (row: WeightedNode[], side: number): number => {
    const sizes = row.map((n) => n.value * scale);
    const sum = sizes.reduce((a, b) => a + b, 0);
    return Math.max(
      (side * side * Math.max(...sizes)) / (sum * sum),
      (sum * sum) / (side * side * Math.min(...sizes)),
    );
  };
  const place = (row: WeightedNode[]): void => {
    const sum = row.reduce((s, n) => s + n.value * scale, 0);
    const vertical = area.width >= area.height;
    const side = vertical ? area.height : area.width;
    const depth = sum / side;
    let offset = 0;
    for (const node of row) {
      const length = (node.value * scale) / depth;
      output.push({
        ...node,
        x: area.x + (vertical ? 0 : offset),
        y: area.y + (vertical ? offset : 0),
        width: vertical ? depth : length,
        height: vertical ? length : depth,
      });
      offset += length;
    }
    area = vertical
      ? { ...area, x: area.x + depth, width: Math.max(0, area.width - depth) }
      : { ...area, y: area.y + depth, height: Math.max(0, area.height - depth) };
  };
  let row: WeightedNode[] = [];
  for (const item of items) {
    const side = Math.min(area.width, area.height);
    if (row.length && worst([...row, item], side) > worst(row, side)) {
      place(row);
      row = [];
    }
    row.push(item);
  }
  if (row.length) place(row);
  return output;
}

const colors = ['#5f91cb', '#d1a358', '#8fae84', '#9c8ec5', '#819eab', '#b0928d'];
export function color(name: string): string {
  let hash = 0;
  for (const char of name) hash = (Math.imul(hash, 31) + char.charCodeAt(0)) | 0;
  return colors[(hash >>> 0) % colors.length] ?? '#819eab';
}
