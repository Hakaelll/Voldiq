import type { IndexId } from './index-context';
import { invoke } from '@tauri-apps/api/core';
import { createQueryRunner, type QueryTransport } from './query-runner';
import type { CloudFilter, FilePage, Overview, NodeId, SelectedDetails, Query } from './types';
import type { BudgetReport } from './types';

let session: Promise<number> | undefined;
function querySession(): Promise<number> {
  session ??= invoke<number>('open_query_session');
  return session;
}

type IndexedInput<T> = T & { indexId: IndexId | null };

const transport: QueryTransport<FilePage, IndexedInput<Query>> = {
  run: async (_family, requestId, { indexId, ...query }) =>
    invoke<FilePage | null>('query_files_cancellable', {
      epoch: await querySession(),
      requestId,
      query,
      indexId,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
};

export const fileQueries = createQueryRunner<FilePage, IndexedInput<Query>>('file_list', transport);
export const detailQueries = createQueryRunner<SelectedDetails, IndexedInput<{ id: NodeId }>>(
  'details',
  {
    run: async (_family, requestId, input) =>
      invoke<SelectedDetails | null>('query_details_cancellable', {
        epoch: await querySession(),
        requestId,
        id: input.id,
        indexId: input.indexId,
      }),
    cancel: transport.cancel,
  },
);
export const overviewQueries = createQueryRunner<
  Overview,
  IndexedInput<{ id: NodeId; cloud: CloudFilter | null; search: Query | null }>
>('overview_children', {
  run: async (_family, requestId, input) =>
    invoke<Overview | null>('query_overview', {
      epoch: await querySession(),
      requestId,
      id: input.id,
      cloud: input.cloud,
      search: input.search,
      indexId: input.indexId,
    }),
  cancel: transport.cancel,
});
export const budgetQueries = createQueryRunner<
  BudgetReport,
  IndexedInput<{ id: NodeId; detailed: boolean }>
>('folder_budget', {
  run: async (_family, requestId, input) =>
    invoke<BudgetReport | null>('query_folder_budget', {
      epoch: await querySession(),
      requestId,
      indexId: input.indexId,
      nodeId: input.id,
      detailed: input.detailed,
    }),
  cancel: transport.cancel,
});
