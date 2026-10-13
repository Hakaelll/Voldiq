import { invoke } from '@tauri-apps/api/core';
import { createQueryRunner, type QueryTransport } from './query-runner';
import type { Page } from './types';

let session: Promise<number> | undefined;
function querySession(): Promise<number> {
  session ??= invoke<number>('open_query_session');
  return session;
}

const transport: QueryTransport = {
  run: async (family, requestId, query) =>
    invoke<Page | null>('query_files_cancellable', {
      epoch: await querySession(),
      family,
      requestId,
      query,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
};

export const fileQueries = createQueryRunner('file_list', transport);
export const overviewQueries = createQueryRunner('overview_children', transport);
