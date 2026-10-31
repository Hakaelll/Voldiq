import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildCity, pointInPolygon, project } from './disk-city.ts';
import type { Face, Vec3 } from './disk-city.ts';
import { prepareShadows } from './map-painter.ts';
import type { Rect } from './storage-map.ts';

function block(
  id: number,
  x: number,
  y: number,
  width: number,
  depth: number,
  height: number,
): Rect {
  return {
    id,
    name: `block-${id}`,
    directory: false,
    value: height,
    x,
    y,
    width,
    height: depth,
    depth: 1,
    modified: null,
    files: 1,
    represented: 1,
    aggregated: 0,
    allocation_known: true,
  };
}

function random(seed: number): () => number {
  let state = seed;
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 4294967296;
  };
}

// Trace toward the sun from a receiver. This independently checks whether
// another closed prism blocks the light before it reaches that surface.
function blocked(point: Vec3, caster: Rect): boolean {
  const direction = { x: 0.65, y: 0.4, z: 1 };
  const min = { x: caster.x, y: caster.y, z: 0 };
  const max = { x: caster.x + caster.width, y: caster.y + caster.height, z: caster.value };
  let enter = 0;
  let leave = Infinity;
  for (const axis of ['x', 'y', 'z'] satisfies (keyof Vec3)[]) {
    enter = Math.max(enter, (min[axis] - point[axis]) / direction[axis]);
    leave = Math.min(leave, (max[axis] - point[axis]) / direction[axis]);
  }
  return leave >= enter && leave > 1e-9;
}

function sampleFace(face: Face, a: number, b: number): Vec3 {
  const first = face.original[0];
  const second = face.original[1];
  const fourth = face.original[3];
  return {
    x: first.x + (second.x - first.x) * a + (fourth.x - first.x) * b,
    y: first.y + (second.y - first.y) * a + (fourth.y - first.y) * b,
    z: first.z + (second.z - first.z) * a + (fourth.z - first.z) * b,
  };
}

function verifyShadowReceivers(nodes: Rect[], seed: number): void {
  const scene = buildCity(nodes, (node) => node.value);
  const shadows = prepareShadows(scene);
  const next = random(seed);
  const checked = new Set<string>();
  for (const face of scene.faces) {
    const key = `${face.node.id}:${face.kind}`;
    if (checked.has(key)) continue;
    checked.add(key);
    assert.ok(shadows.has(key));
    for (let index = 0; index < 35; index++) {
      const point = sampleFace(face, 0.001 + next() * 0.998, 0.001 + next() * 0.998);
      const expected = nodes.some((node) => node.id !== face.node.id && blocked(point, node));
      const actual = (shadows.get(key) ?? []).some((shape) =>
        pointInPolygon(project(point), shape),
      );
      assert.equal(
        actual,
        expected,
        `receiver ${key}, point ${JSON.stringify(point)}, seed ${seed}`,
      );
    }
  }
  const minX = Math.min(...nodes.map((node) => node.x - node.value * 0.65)) - 10;
  const minY = Math.min(...nodes.map((node) => node.y - node.value * 0.4)) - 10;
  const maxX = Math.max(...nodes.map((node) => node.x + node.width)) + 10;
  const maxY = Math.max(...nodes.map((node) => node.y + node.height)) + 10;
  for (let index = 0; index < 300; index++) {
    const point = { x: minX + next() * (maxX - minX), y: minY + next() * (maxY - minY), z: 0 };
    const actual = nodes.some((node) =>
      (shadows.get(`ground:${node.id}`) ?? []).some((shape) =>
        pointInPolygon(project(point), shape),
      ),
    );
    assert.equal(
      actual,
      nodes.some((node) => blocked(point, node)),
      `ground point ${JSON.stringify(point)}`,
    );
  }
}

test('physical shadow silhouettes cover roofs and both visible facades', () => {
  const nodes = [
    block(1, 0, 0, 20, 20, 15),
    block(2, 25, 10, 15, 12, 45),
    block(3, 10, 25, 12, 15, 60),
  ];
  const shadows = prepareShadows(buildCity(nodes, (node) => node.value));
  const samples: { kind: Face['kind']; point: Vec3 }[] = [
    { kind: 'roof', point: { x: 10, y: 12, z: 15 } },
    { kind: 'east', point: { x: 20, y: 14, z: 10 } },
    { kind: 'south', point: { x: 8, y: 20, z: 10 } },
  ];
  for (const { kind, point } of samples) {
    assert.ok(nodes.slice(1).some((node) => blocked(point, node)));
    assert.ok(
      (shadows.get(`1:${kind}`) ?? []).some((shape) => pointInPolygon(project(point), shape)),
      kind,
    );
  }
  verifyShadowReceivers(nodes, 117);
});

test('shadow broadphase preserves exact light-ray results across grid boundaries and negative coordinates', () => {
  for (let seed = 1; seed <= 15; seed++) {
    const next = random(seed * 571);
    const offsetX = seed % 3 === 0 ? -700 : seed % 3 === 1 ? 139.9 : 0;
    const offsetY = seed % 2 === 0 ? -281 : 140.1;
    const nodes = Array.from({ length: 30 }, (_, id) =>
      block(
        id,
        offsetX + (id % 6) * 23,
        offsetY + Math.floor(id / 6) * 19,
        2 + next() * 20,
        2 + next() * 16,
        1 + next() * 260,
      ),
    );
    verifyShadowReceivers(nodes, seed * 137);
  }
});

test('a caster straddling the receiver support plane casts only from its upstream portion', () => {
  const receiver = block(1, 0, 0, 20, 20, 40);
  const caster = block(2, 15, 25, 20, 12, 65);
  verifyShadowReceivers([receiver, caster], 719);
});

test('shadow preparation is independent of BSP fragment count and excludes self casting', () => {
  const nodes = [
    block(1, 0, 0, 80, 20, 73),
    block(2, 80, 0, 20, 80, 53),
    block(3, 20, 80, 80, 20, 97),
    block(4, 0, 20, 20, 80, 41),
    block(5, 20, 20, 60, 60, 61),
  ];
  const scene = buildCity(nodes, (node) => node.value);
  assert.ok(scene.faces.length > nodes.length * 3);
  const shadows = prepareShadows(scene);
  assert.equal(shadows.size, nodes.length * 4);
  verifyShadowReceivers(nodes, 873);
  const alone = prepareShadows(buildCity([block(1, 0, 0, 50, 50, 20)], (node) => node.value));
  assert.deepEqual(alone.get('1:roof'), []);
  assert.deepEqual(alone.get('1:east'), []);
  assert.deepEqual(alone.get('1:south'), []);
});
