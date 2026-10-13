import { invoke } from '@tauri-apps/api/core';
import { createQueryRunner, type QueryTransport } from './query-runner';
import type { Page, Overview, NodeId, Details } from './types';

let session: Promise<number> | undefined;
function querySession(): Promise<number> {
  session ??= invoke<number>('open_query_session');
  return session;
}

const transport: QueryTransport = {
  run: async (_family, requestId, query) =>
    invoke<Page | null>('query_files_cancellable', {
      epoch: await querySession(),
      requestId,
      query,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
};

export const fileQueries = createQueryRunner('file_list', transport);
export const detailQueries = createQueryRunner<Details, { id: NodeId }>('details', {
  run: async (_family, requestId, input) =>
    invoke<Details | null>('query_details_cancellable', {
      epoch: await querySession(), requestId, id: input.id,
    }),
  cancel: transport.cancel,
});
export const overviewQueries = createQueryRunner<Overview, { id: NodeId }>('overview_children', {
  run: async (_family, requestId, input) =>
    invoke<Overview | null>('query_overview', {
      epoch: await querySession(),
      requestId,
      id: input.id,
    }),
  cancel: transport.cancel,
});
