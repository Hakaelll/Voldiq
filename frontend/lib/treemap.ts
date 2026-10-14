export interface WeightedNode {
  id: number;
  name: string;
  value: number;
  directory: boolean;
  children?: WeightedNode[];
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

export function nestedLayout(nodes: WeightedNode[], width: number, height: number): Rect[] {
  const output: Rect[] = [];
  for (const parent of layout(nodes, width, height)) {
    output.push(parent);
    const children =
      parent.children?.filter((item) => Number.isFinite(item.value) && item.value > 0) ?? [];
    const sum = children.reduce((total, item) => total + item.value, 0);
    if (!children.length || sum > parent.value || parent.width < 150 || parent.height < 140)
      continue;
    const remainder = parent.value - sum;
    if (remainder > 0)
      children.push({ id: parent.id, name: 'Other', value: remainder, directory: true });
    for (const child of layout(children, parent.width - 12, parent.height - 58)) {
      output.push({ ...child, x: child.x + parent.x + 6, y: child.y + parent.y + 52 });
    }
  }
  return output;
}

const colors = [
  { fill: '#075bdd', ink: '#ffffff' },
  { fill: '#123f86', ink: '#ffffff' },
  { fill: '#087f8c', ink: '#ffffff' },
  { fill: '#ed701b', ink: '#101820' },
  { fill: '#c59127', ink: '#101820' },
  { fill: '#717a84', ink: '#000000' },
];
export function mapColor(name: string): { fill: string; ink: string } {
  let hash = 0;
  for (const char of name) hash = (Math.imul(hash, 31) + char.charCodeAt(0)) | 0;
  return colors[(hash >>> 0) % colors.length] ?? { fill: '#717a84', ink: '#000000' };
}
export function color(name: string): string {
  return mapColor(name).fill;
}
