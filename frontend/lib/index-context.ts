import { invoke } from '@tauri-apps/api/core';

export type IndexId = number & { readonly __brand: 'IndexId' };
let current: IndexId | null = null;

export function setIndexContext(value: number | null): IndexId | null {
  if (value !== null && (!Number.isSafeInteger(value) || value <= 0)) {
    throw new Error('Invalid analysis identity.');
  }
  current = value as IndexId | null;
  return current;
}

export function getIndexContext(): IndexId | null {
  return current;
}

export async function invokeIndexed<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const indexId = current;
  if (indexId === null && command !== 'query_cleanup_rule') throw new Error('Scan a drive or folder first.');
  const result = await invoke<T>(command, { ...args, indexId });
  if (current !== indexId) throw new Error('Analysis changed. Refresh the view before continuing.');
  return result;
}
