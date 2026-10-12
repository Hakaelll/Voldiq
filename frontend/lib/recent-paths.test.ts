import assert from 'node:assert/strict';
import test from 'node:test';
import { readRecentPaths, rememberPath } from './recent-paths.ts';

test('recent paths survive serialization, revisit moves to front, and oldest paths expire', () => {
  let paths: string[] = [];
  for (let index = 0; index < 12; index++) paths = rememberPath(paths, `C:\\project-${index}`);
  assert.equal(paths.length, 10);
  assert.equal(paths[0], 'C:\\project-11');
  assert.equal(paths.at(-1), 'C:\\project-2');
  paths = rememberPath(paths, 'C:\\project-5');
  assert.equal(paths[0], 'C:\\project-5');
  assert.equal(paths.filter((path) => path === 'C:\\project-5').length, 1);
  assert.deepEqual(readRecentPaths(JSON.parse(JSON.stringify(paths))), paths);
});

test('stored paths reject malformed entries while preserving exact Unicode and case-sensitive names', () => {
  assert.deepEqual(readRecentPaths(null), []);
  assert.deepEqual(readRecentPaths({ path: 'C:\\folder' }), []);
  const paths = ['C:\\A', 'C:\\a', 'D:\\照片', '\\\\server\\share\\folder'];
  assert.deepEqual(
    readRecentPaths([false, 4, null, '', '\0bad', 'x'.repeat(32768), ...paths, paths[0]]),
    paths,
  );
});
