import { test } from 'node:test';
import assert from 'node:assert/strict';
import { startVoronoiJob, type VoronoiPort } from './voronoi-job.ts';
import { computeVoronoi, type VoronoiReply } from './voronoi.ts';

function fixture() {
  let reply: ((value: VoronoiReply) => void) | undefined,
    failed: (() => void) | undefined,
    terminated = 0;
  const port: VoronoiPort = {
    post(_input, answer, error) {
      reply = answer;
      failed = error;
    },
    terminate() {
      terminated++;
    },
  };
  return {
    port,
    reply: (value: VoronoiReply) => reply?.(value),
    fail: () => failed?.(),
    terminated: () => terminated,
  };
}
const ready: VoronoiReply = { kind: 'ready', layout: computeVoronoi([], 0) };

test('one Voronoi worker result wins and disposal rejects queued replies', () => {
  const first = fixture(),
    results: VoronoiReply[] = [];
  const close = startVoronoiJob([], 0, (value) => results.push(value), {
    create: () => first.port,
  });
  first.reply(ready);
  first.fail();
  first.reply(ready);
  close();
  assert.deepEqual([...results], [ready]);
  assert.equal(first.terminated(), 1);
  const second = fixture();
  const cancel = startVoronoiJob([], 0, (value) => results.push(value), {
    create: () => second.port,
  });
  cancel();
  second.reply(ready);
  second.fail();
  cancel();
  assert.deepEqual([...results], [ready]);
  assert.equal(second.terminated(), 1);
});

test('Voronoi timeout terminates the worker and a late ready result cannot revive it', async () => {
  const source = fixture();
  const replies: VoronoiReply[] = [];
  const result = await new Promise<VoronoiReply>((resolve) =>
    startVoronoiJob(
      [],
      0,
      (value) => {
        replies.push(value);
        resolve(value);
      },
      { create: () => source.port, timeoutMs: 1 },
    ),
  );
  assert.equal(result.kind, 'error');
  if (result.kind === 'error') assert.match(result.message, /time limit/);
  source.reply(ready);
  source.fail();
  assert.equal(replies.length, 1);
  assert.equal(source.terminated(), 1);
});

test('worker construction and runtime failures are explicit terminal results', () => {
  const results: VoronoiReply[] = [];
  startVoronoiJob([], 0, (value) => results.push(value), {
    create: () => {
      throw new Error('fixture unavailable');
    },
  })();
  const source = fixture();
  const close = startVoronoiJob([], 0, (value) => results.push(value), {
    create: () => source.port,
  });
  source.fail();
  source.reply(ready);
  close();
  assert.equal(results.length, 2);
  assert.ok(results.every((reply) => reply.kind === 'error'));
  assert.equal(source.terminated(), 1);
});
