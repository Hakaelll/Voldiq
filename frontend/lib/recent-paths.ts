const limit = 10;

export function readRecentPaths(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const paths: string[] = [];
  for (const path of value) {
    if (
      typeof path === 'string' &&
      path.length > 0 &&
      path.length <= 32767 &&
      !path.includes('\0') &&
      !paths.includes(path)
    )
      paths.push(path);
    if (paths.length === limit) break;
  }
  return paths;
}

export function rememberPath(paths: readonly string[], path: string): string[] {
  return readRecentPaths([path, ...paths]);
}
