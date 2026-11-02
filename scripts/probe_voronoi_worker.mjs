import { Worker } from 'node:worker_threads';
import { randomUUID, createHash } from 'node:crypto';
import { mkdir, readdir, readFile, writeFile, unlink, rmdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createStorageLayout } from '../frontend/lib/storage-map.ts';

const repository = fileURLToPath(new URL('../', import.meta.url));
const assets = path.join(repository, 'dist/assets');
const names = (await readdir(assets)).filter((name) => /^voronoi\.worker-[\w-]+\.js$/.test(name));
if (names.length !== 1)
  throw new Error('Build exactly one production Voronoi worker before probing.');
const asset = path.join(assets, names[0]);
const directory = path.join(repository, 'target/probes', `voronoi-worker-${randomUUID()}`);
await mkdir(directory, { recursive: true });
const host = path.join(directory, 'host.mjs');
await writeFile(
  host,
  `import {parentPort,workerData} from 'node:worker_threads';
globalThis.self={postMessage:value=>parentPort.postMessage(value),onmessage:null};
await import(workerData);
parentPort.on('message',data=>globalThis.self.onmessage({data}));
parentPort.postMessage({hostReady:true});
`,
);
const source = createStorageLayout(
  Array.from({ length: 250 }, (_, id) => ({ id, name: String(id), value: 1, directory: false })),
);
let worker;
try {
  worker = new Worker(pathToFileURL(host), { workerData: pathToFileURL(asset).href });
  const reply = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Owned worker fixture timed out.')), 10000);
    worker.on('error', (error) => {
      clearTimeout(timer);
      reject(error);
    });
    worker.on('exit', (code) => {
      if (code !== 0) {
        clearTimeout(timer);
        reject(new Error(`Worker exited ${code}.`));
      }
    });
    worker.on('message', (message) => {
      if (message.hostReady) worker.postMessage({ rects: source.rects, total: source.total });
      else {
        clearTimeout(timer);
        resolve(message);
      }
    });
  });
  if (
    reply.kind !== 'ready' ||
    reply.layout.cells.length !== 120 ||
    reply.layout.groupedEntries !== 131 ||
    !Number.isFinite(reply.layout.maximumError) ||
    !Number.isFinite(reply.layout.totalError) ||
    reply.layout.maximumError > 0.05 ||
    reply.layout.totalError > 0.002
  )
    throw new Error('Production worker did not produce an acceptable weighted diagram.');
  const members = reply.layout.cells.flatMap((cell) => cell.members).sort((a, b) => a - b);
  if (members.length !== 250 || members.some((id, index) => id !== index))
    throw new Error('Production worker lost or duplicated fixture entries.');
  let observedError = 0,
    observedMaximum = 0;
  for (const cell of reply.layout.cells) {
    if (cell.node.value !== cell.members.length)
      throw new Error('Worker changed fixture byte totals.');
    const measured =
      Math.abs(
        cell.polygon.reduce((sum, p, index) => {
          const q = cell.polygon[(index + 1) % cell.polygon.length];
          return sum + p.x * q.y - p.y * q.x;
        }, 0),
      ) / 1_400_000;
    const expected = cell.members.length / 250;
    observedError += Math.abs(measured - expected) / 2;
    observedMaximum = Math.max(observedMaximum, Math.abs(measured - expected) / expected);
  }
  if (
    !Number.isFinite(observedMaximum) ||
    observedError > 0.002 ||
    observedMaximum > 0.05 ||
    Math.abs(observedError - reply.layout.totalError) > 1e-8 ||
    Math.abs(observedMaximum - reply.layout.maximumError) > 1e-8
  )
    throw new Error('Worker polygon areas do not match their reported precision.');
  const result = {
    checkedUtc: new Date().toISOString(),
    asset: `dist/assets/${names[0]}`,
    sha256: createHash('sha256')
      .update(await readFile(asset))
      .digest('hex'),
    sourceEntries: 250,
    cells: 120,
    groupedEntries: 131,
    totalError: reply.layout.totalError,
    maximumError: reply.layout.maximumError,
    observedAreaDiscrepancy: observedError,
    observedMaximumError: observedMaximum,
    clips: reply.layout.clips,
    scope:
      'Exact production browser-worker asset executed in an owned Node worker thread through a self/postMessage adapter. Synthetic metadata only. This proves the built worker protocol and geometry, not WebView creation, native rendering, UI interaction or device performance.',
  };
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
} finally {
  if (worker) await worker.terminate();
  await unlink(host);
  await rmdir(directory);
}
