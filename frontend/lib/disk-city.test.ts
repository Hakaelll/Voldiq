import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  buildCity,
  createPicker,
  fitCamera,
  pickFace,
  pointInPolygon,
  project,
  screenToWorld,
  worldToScreen,
  zoomCamera,
} from './disk-city.ts';
import type { Face, Point } from './disk-city.ts';
import type { Rect } from './treemap.ts';

function rect(
  id: number,
  x: number,
  y: number,
  width: number,
  depth: number,
  height: number,
): Rect {
  return { id, name: `box-${id}`, value: height, directory: false, x, y, width, height: depth };
}

function random(seed: number): () => number {
  let state = seed;
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 4294967296;
  };
}

// Independently intersect the projection's viewing ray with each solid box.
// The visible hit has the greatest ray parameter toward the (+1,+1,+1) eye.
function rayHit(nodes: Rect[], point: Point): { node: Rect; kind: Face['kind'] } | null {
  const baseX = point.y + point.x / Math.sqrt(3);
  const baseY = point.y - point.x / Math.sqrt(3);
  let nearest = -Infinity;
  let hit: { node: Rect; kind: Face['kind'] } | null = null;
  for (const node of nodes) {
    const enter = Math.max(node.x - baseX, node.y - baseY, 0);
    const east = node.x + node.width - baseX;
    const south = node.y + node.height - baseY;
    const leave = Math.min(east, south, node.value);
    if (enter > leave + 1e-10 || leave <= nearest) continue;
    nearest = leave;
    hit = { node, kind: leave === node.value ? 'roof' : leave === east ? 'east' : 'south' };
  }
  return hit;
}

function comparePicker(nodes: Rect[], seed: number, count: number): void {
  const city = buildCity(nodes, (node) => node.value);
  const pick = createPicker(city.faces);
  const next = random(seed);
  assert.equal(city.fragments, city.faces.length);
  assert.ok(city.fragments <= Math.max(12_000, nodes.length * 96));
  for (let index = 0; index < count; index++) {
    const point = {
      x: city.bounds.x + (next() * 1.1 - 0.05) * city.bounds.width,
      y: city.bounds.y + (next() * 1.1 - 0.05) * city.bounds.height,
    };
    const expected = rayHit(nodes, point);
    const actual = pick(point);
    assert.equal(
      actual?.node.id ?? null,
      expected?.node.id ?? null,
      `seed ${seed}, point ${JSON.stringify(point)}`,
    );
    assert.equal(
      actual?.kind ?? null,
      expected?.kind ?? null,
      `face at seed ${seed}, point ${JSON.stringify(point)}`,
    );
    if (index < 15) assert.equal(pickFace(city.faces, point), actual);
  }
}

test('BSP painting and binned facade picking agree with analytic ray-box intersections', () => {
  assert.equal(createPicker([])({ x: 0, y: 0 }), null);
  assert.equal(pickFace([], { x: 0, y: 0 }), null);
  const strips = Array.from({ length: 35 }, (_, index) =>
    rect(index, index * 2.1, 0, 2, 95, ((index * 31) % 83) + 0.02),
  );
  const crossStrips = Array.from({ length: 35 }, (_, index) =>
    rect(index, 0, index * 2.1, 95, 2, ((index * 47) % 103) + 0.01),
  );
  const thin = Array.from({ length: 55 }, (_, index) =>
    rect(index, index * 0.009, -10, 0.003, 180, ((index * 13) % 171) + 1),
  );
  const grid = Array.from({ length: 144 }, (_, index) =>
    rect(index, (index % 12) * 5, Math.floor(index / 12) * 5, 4.8, 4.8, ((index * 29) % 100) + 0.1),
  );
  const spiral: Rect[] = [];
  let x = -80;
  let y = -80;
  let width = 160;
  let depth = 160;
  for (let index = 0; index < 40; index++) {
    if (index % 4 === 0) {
      spiral.push(rect(index, x, y, width, 3, ((index * 59) % 190) + 1));
      y += 3;
      depth -= 3;
    } else if (index % 4 === 1) {
      spiral.push(rect(index, x + width - 3, y, 3, depth, ((index * 59) % 190) + 1));
      width -= 3;
    } else if (index % 4 === 2) {
      spiral.push(rect(index, x, y + depth - 3, width, 3, ((index * 59) % 190) + 1));
      depth -= 3;
    } else {
      spiral.push(rect(index, x, y, 3, depth, ((index * 59) % 190) + 1));
      x += 3;
      width -= 3;
    }
  }
  const adverse = [
    rect(1, -70, 0, 130, 1, 5),
    rect(2, 60.1, -60, 1, 130, 150),
    rect(3, -60, 2, 100, 0.01, 70),
    rect(4, -50, 4, 2, 100, 230),
    rect(5, -40, 6, 90, 0.02, 10),
    rect(6, 41, 8, 0.01, 100, 260),
  ];
  [strips, crossStrips, thin, grid, spiral, adverse].forEach((nodes, index) =>
    comparePicker(nodes, index + 71, 2200),
  );
});

test('seeded nonoverlapping guillotine tilings preserve visibility for arbitrary long and tall buildings', () => {
  for (let seed = 1; seed <= 35; seed++) {
    const next = random(seed * 1327);
    const leaves = [{ x: -37, y: -54, width: 120, height: 170 }];
    while (leaves.length < 50) {
      const index = Math.floor(next() * leaves.length);
      const leaf = leaves[index];
      const fraction = 0.08 + next() * 0.84;
      if (next() > 0.5) {
        const width = leaf.width * fraction;
        leaves.splice(
          index,
          1,
          { ...leaf, width },
          { ...leaf, x: leaf.x + width, width: leaf.width - width },
        );
      } else {
        const height = leaf.height * fraction;
        leaves.splice(
          index,
          1,
          { ...leaf, height },
          { ...leaf, y: leaf.y + height, height: leaf.height - height },
        );
      }
    }
    const nodes = leaves.map((leaf, id) =>
      rect(id, leaf.x, leaf.y, leaf.width, leaf.height, next() * 280 + 0.01),
    );
    comparePicker(nodes, seed * 91, 500);
  }
});

test('split fragments retain complete original faces and preserve their projected area', () => {
  const nodes = [
    rect(1, 0, 0, 80, 20, 73),
    rect(2, 80, 0, 20, 80, 53),
    rect(3, 20, 80, 80, 20, 97),
    rect(4, 0, 20, 20, 80, 41),
    rect(5, 20, 20, 60, 60, 61),
  ];
  const city = buildCity(nodes, (node) => node.value);
  assert.ok(city.fragments > nodes.length * 3, 'fixture exercises actual polygon subdivision');
  function area(vertices: Point[]): number {
    return (
      Math.abs(
        vertices.reduce((sum, point, index) => {
          const next = vertices[(index + 1) % vertices.length];
          return sum + point.x * next.y - next.x * point.y;
        }, 0),
      ) / 2
    );
  }
  const byOriginal = new Map<Face['original'], number>();
  for (const face of city.faces) {
    assert.equal(face.original.length, 4);
    assert.ok(nodes.includes(face.node));
    byOriginal.set(
      face.original,
      (byOriginal.get(face.original) ?? 0) + area(face.vertices.map(project)),
    );
  }
  assert.equal(byOriginal.size, nodes.length * 3);
  for (const [original, fragmentsArea] of byOriginal) {
    const expected = area(original.map(project));
    assert.ok(Math.abs(fragmentsArea - expected) < Math.max(1, expected) * 1e-9);
  }
});

test('full elevated geometry fits the viewport and zoom keeps the cursor anchored at both clamps', () => {
  assert.ok(Number.isFinite(fitCamera({ x: 0, y: 0, width: 0, height: 0 }, 0, 0).scale));
  assert.throws(() => buildCity([rect(1, 0, 0, 10, 10, 1)], () => NaN), /Invalid disk city prism/);
  const nodes = [rect(1, -80, -20, 170, 4, 350), rect(2, -70, 0, 20, 180, 500)];
  const city = buildCity(nodes, (node) => node.value);
  const camera = fitCamera(city.bounds, 920, 610);
  for (const face of city.faces) {
    for (const vertex of face.vertices) {
      const screen = worldToScreen(project(vertex), camera);
      assert.ok(screen.x >= 24 - 1e-9 && screen.x <= 896 + 1e-9);
      assert.ok(screen.y >= 24 - 1e-9 && screen.y <= 586 + 1e-9);
    }
  }
  assert.ok(city.bounds.y < -400);
  const cursor = { x: 287, y: 143 };
  const anchored = screenToWorld(cursor, camera);
  for (const factor of [0.0001, 0.7, 2, 10_000]) {
    const zoomed = zoomCamera(camera, cursor, factor, camera.scale / 2, camera.scale * 8);
    const projected = worldToScreen(anchored, zoomed);
    assert.ok(Math.abs(projected.x - cursor.x) < 1e-9);
    assert.ok(Math.abs(projected.y - cursor.y) < 1e-9);
    assert.ok(zoomed.scale >= camera.scale / 2 && zoomed.scale <= camera.scale * 8);
  }
});

test('a thousand-building tiling stays under the geometry fragment budget', () => {
  const nodes = Array.from({ length: 1000 }, (_, id) =>
    rect(id, (id % 40) * 2, Math.floor(id / 40) * 2, 1.98, 1.98, ((id * 971) % 103) + 0.01),
  );
  const city = buildCity(nodes, (node) => node.value);
  assert.ok(city.fragments < 32 * 3 * nodes.length);
  const pick = createPicker(city.faces);
  assert.equal(pick({ x: -100_000, y: 100_000 }), null);
});
