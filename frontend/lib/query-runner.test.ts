import assert from 'node:assert/strict';
import test from 'node:test';
import { createQueryRunner, type QueryFamily, type QueryTransport } from './query-runner.ts';
import type { Page, Query } from './types.ts';

function deferred<T = Page>() {
  let resolve = (_page: T | null) => {};
  let reject = (_problem: unknown) => {};
  const result = new Promise<T | null>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return {
    result,
    resolve: (page: T | null) => resolve(page),
    reject: (problem: unknown) => reject(problem),
  };
}

function fixture() {
  const runs: {
    family: QueryFamily;
    id: number;
    query: Query;
    completion: ReturnType<typeof deferred<Page>>;
  }[] = [];
  const cancellations: number[] = [];
  const transport: QueryTransport = {
    run: (family, id, query) => {
      const completion = deferred();
      runs.push({ family, id, query, completion });
      return completion.result;
    },
    cancel: async (_family, id) => {
      cancellations.push(id);
    },
  };
  return { transport, runs, cancellations };
}

const flush = () => new Promise<void>((done) => setImmediate(done));

test('detail supersession and panel cancellation leave file queries running and cannot reopen old details', async () => {
  const { transport, runs } = fixture();
  const files = createQueryRunner('file_list', transport);
  const fileRequest = files.request({});
  const completions = [deferred<{ path: string }>(), deferred<{ path: string }>(), deferred<{ path: string }>()];
  const scopes: number[] = [];
  const cancellations: number[] = [];
  const details = createQueryRunner<{ path: string }, { id: number }>('details', {
    run: (family, _request, query) => {
      assert.equal(family, 'details');
      scopes.push(query.id);
      return completions[scopes.length - 1].result;
    },
    cancel: async (family, id) => {
      assert.equal(family, 'details');
      cancellations.push(id);
    },
  });
  const old = details.request({ id: 42 });
  await flush();
  const next = details.request({ id: 7 });
  assert.equal(await old.result, null);
  completions[0].reject(new Error('obsolete details'));
  await flush();
  details.cancel();
  assert.equal(await next.result, null);
  completions[1].resolve({ path: 'closed panel' });
  await flush();
  const reopened = details.request({ id: 8 });
  await flush();
  old.cancel();
  assert.equal(cancellations.length, 2);
  completions[2].resolve({ path: 'current item' });
  assert.deepEqual(await reopened.result, { path: 'current item' });
  assert.deepEqual(scopes, [42, 7, 8]);
  runs[0].completion.resolve({ items: [], total: 3 });
  assert.deepEqual(await fileRequest.result, { items: [], total: 3 });
});

test('summary requests keep typed scope input and discard the obsolete composite response', async () => {
  const first = deferred<{ types: number }>();
  const second = deferred<{ types: number }>();
  const scopes: number[] = [];
  const runner = createQueryRunner<{ types: number }, { id: number }>('overview_children', {
    run: (_family, _request, query) => {
      scopes.push(query.id);
      return (scopes.length === 1 ? first : second).result;
    },
    cancel: async () => {},
  });
  const obsolete = runner.request({ id: 42 });
  await flush();
  const latest = runner.request({ id: 7 });
  assert.equal(await obsolete.result, null);
  first.resolve({ types: 999 });
  await flush();
  assert.deepEqual(scopes, [42, 7]);
  second.resolve({ types: 3 });
  assert.deepEqual(await latest.result, { types: 3 });
});

test('superseding requests cancel current work and queue only the latest query', async () => {
  const { transport, runs, cancellations } = fixture();
  const runner = createQueryRunner('file_list', transport);
  const first = runner.request({ text: 'first' });
  await flush();
  const replaced = runner.request({ text: 'replaced' });
  const latest = runner.request({ text: 'latest' });
  assert.equal(await first.result, null);
  assert.equal(await replaced.result, null);
  assert.equal(runs.length, 1);
  assert.deepEqual(cancellations, [runs[0].id]);
  runs[0].completion.resolve({ total: 999, items: [] });
  await flush();
  assert.equal(runs.length, 2);
  assert.equal(runs[1].query.text, 'latest');
  assert.ok(runs[1].id > runs[0].id);
  const page: Page = { total: 3, items: [] };
  runs[1].completion.resolve(page);
  assert.equal(await latest.result, page);
});

test('unmount cancellation suppresses late errors and an old ticket cannot cancel a newer one', async () => {
  const { transport, runs, cancellations } = fixture();
  const runner = createQueryRunner('file_list', transport);
  const old = runner.request({});
  await flush();
  old.cancel();
  assert.equal(await old.result, null);
  const next = runner.request({ text: 'new mount' });
  runs[0].completion.reject(new Error('obsolete IPC failure'));
  await flush();
  old.cancel();
  assert.deepEqual(cancellations, [runs[0].id]);
  runner.cancel();
  assert.equal(await next.result, null);
  runs[1].completion.resolve({ total: 0, items: [] });
  await flush();
  assert.equal(runs.length, 2);
});

test('families run independently and a current transport failure reaches its caller', async () => {
  const { transport, runs } = fixture();
  const files = createQueryRunner('file_list', transport);
  const overview = createQueryRunner('overview_children', transport);
  const fileRequest = files.request({});
  const mapRequest = overview.request({ parent: 0 });
  await flush();
  const failure = assert.rejects(mapRequest.result, /current failure/);
  runs[1].completion.reject(new Error('current failure'));
  await failure;
  files.cancel();
  assert.equal(await fileRequest.result, null);
  runs[0].completion.resolve(null);
  await flush();
});
