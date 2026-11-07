import type { HistoryClip, HistoryClipRequest } from './history-clip';
import type { IndexId } from './index-context';
import type { SpaceReport } from './space-account';
import type { RecoveryReport } from './recovery';
import type { DeviceHealthReport } from './device-health';
import type { GameLibraryReport } from './game-libraries';
import type { MediaLibraryReport } from './media-libraries';
import type { ApplicationQuery, ApplicationCleanupContext } from './applications';
import type { TrayGroup, TraySummary, TrayTarget } from './cleanup-tray';
import { invoke } from '@tauri-apps/api/core';
import { createQueryRunner, type QueryTransport } from './query-runner';
import type { CloudFilter, FilePage, Overview, NodeId, SelectedDetails, Query } from './types';
import type {
  BudgetReport,
  SnapshotVisualPair,
  SnapshotComparison,
  SnapshotVisualFrame,
  SnapshotRetentionPreview,
  CalendarReport,
  CalendarPage,
  DiaryPage,
} from './types';

let session: Promise<number> | undefined;
function querySession(): Promise<number> {
  session ??= invoke<number>('open_query_session');
  return session;
}

export type InspectionRequest = 'preview' | 'download_source' | 'download_url';

export type FileInspection =
  | { kind: 'metadata' | 'download_unavailable'; reason: string }
  | { kind: 'text'; text: string; truncated: boolean }
  | { kind: 'image'; width: number; height: number; png: string }
  | {
      kind: 'archive';
      format: 'zip' | 'jar' | 'tar' | 'tgz' | 'iso';
      items: { path: string; kind: 'file' | 'folder' | 'link'; logical_bytes: string | null }[];
      omitted: number;
      limit: 'records' | 'bytes' | null;
    }
  | { kind: 'download_source'; domain: string }
  | { kind: 'download_url'; url: string };

export const inspectionQueries = createQueryRunner<
  FileInspection,
  { indexId: IndexId; id: NodeId; inspection: InspectionRequest },
  'file_inspection'
>('file_inspection', {
  run: async (_family, requestId, input) =>
    invoke<FileInspection | null>('query_file_inspection', {
      ...input,
      epoch: await querySession(),
      requestId,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
});

export type GitStorageReport = {
  total: number;
  coverage_partial: boolean;
  git_file_markers: number;
  shared_store_markers: number;
  items: {
    id: NodeId;
    store_id: NodeId;
    path: string;
    loose_bytes: string;
    pack_bytes: string;
    pack_metadata_bytes: string;
    lfs_bytes: string | null;
    total_bytes: string;
    partial: boolean;
    alternates: boolean;
  }[];
};

export const gitStorageQueries = createQueryRunner<
  GitStorageReport,
  { indexId: IndexId; query: { scope: NodeId; offset: number; limit: number } },
  'git_storage'
>('git_storage', {
  run: async (_family, requestId, input) =>
    invoke<GitStorageReport | null>('query_git_storage', {
      ...input,
      epoch: await querySession(),
      requestId,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
});

export const gameLibraryQueries = createQueryRunner<
  GameLibraryReport,
  { indexId: IndexId; query: { scope: NodeId; offset: number; limit: number; manual: string[] } },
  'game_libraries'
>('game_libraries', {
  run: async (_family, requestId, query) =>
    invoke<GameLibraryReport | null>('query_game_libraries', {
      ...query,
      epoch: await querySession(),
      requestId,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
});

export const mediaLibraryQueries = createQueryRunner<
  MediaLibraryReport,
  { indexId: IndexId; query: { scope: NodeId; offset: number; limit: number; manual: string[] } },
  'media_libraries'
>('media_libraries', {
  run: async (_family, requestId, query) =>
    invoke<MediaLibraryReport | null>('query_media_libraries', {
      ...query,
      epoch: await querySession(),
      requestId,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
});

export const cleanupTrayQueries = createQueryRunner<
  TraySummary,
  { indexId: IndexId; groups: readonly TrayGroup[]; target: TrayTarget | null },
  'cleanup_tray'
>('cleanup_tray', {
  run: async (_family, requestId, input) =>
    invoke<TraySummary | null>('query_cleanup_tray', {
      ...input,
      epoch: await querySession(),
      requestId,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
});

export const applicationQueries = createQueryRunner<
  unknown,
  { indexId: IndexId; query: ApplicationQuery },
  'applications'
>('applications', {
  run: async (_family, requestId, input) =>
    invoke<unknown>('query_applications', {
      ...input,
      epoch: await querySession(),
      requestId,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', {
      epoch: await querySession(),
      family,
      requestId,
    }),
});

export const applicationCacheQueries = createQueryRunner<
  unknown,
  {
    indexId: IndexId;
    request: {
      context: ApplicationCleanupContext;
      offset: number;
      limit: number;
      weights: import('./review-priority').ReviewWeights;
    };
  },
  'applications'
>('applications', {
  run: async (_family, requestId, input) =>
    invoke<unknown>('query_application_cache', {
      ...input,
      epoch: await querySession(),
      requestId,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', {
      epoch: await querySession(),
      family,
      requestId,
    }),
});

export const recoveryQueries = createQueryRunner<RecoveryReport, { indexId: IndexId }>(
  'recovery_copies',
  {
    run: async (_family, requestId, query) =>
      invoke<RecoveryReport | null>('query_recovery_copies', {
        ...query,
        epoch: await querySession(),
        requestId,
      }),
    cancel: async (family, requestId) =>
      invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
  },
);

export const deviceHealthQueries = createQueryRunner<
  DeviceHealthReport,
  { indexId: IndexId },
  'device_health'
>('device_health', {
  run: async (_family, requestId, query) =>
    invoke<DeviceHealthReport | null>('query_device_health', {
      ...query,
      epoch: await querySession(),
      requestId,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
});

export const spaceAccountQueries = createQueryRunner<SpaceReport, { indexId: IndexId }>(
  'space_account',
  {
    run: async (_family, requestId, query) =>
      invoke<SpaceReport | null>('query_space_account', {
        ...query,
        epoch: await querySession(),
        requestId,
      }),
    cancel: async (family, requestId) =>
      invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
  },
);

export const snapshotVisualQueries = createQueryRunner<
  SnapshotVisualPair,
  {
    before: number;
    after: number;
    scope: string | null;
    folderCursor: string | null;
    reverse: boolean;
  }
>('snapshot_visual', {
  run: async (_family, requestId, query) =>
    invoke<SnapshotVisualPair | null>('query_snapshot_visual', {
      query,
      epoch: await querySession(),
      requestId,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
});

type IndexedInput<T> = T & { indexId: IndexId | null };

export const calendarQueries = createQueryRunner<
  CalendarReport,
  { snapshot: number; month: string }
>('snapshot_calendar', {
  run: async (_family, requestId, input) =>
    invoke<CalendarReport | null>('query_snapshot_calendar', {
      ...input,
      epoch: await querySession(),
      requestId,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
});
export const calendarEntryQueries = createQueryRunner<
  CalendarPage,
  { snapshot: number; month: string; day: number; offset: number; limit: number }
>('snapshot_calendar_entries', {
  run: async (_family, requestId, query) =>
    invoke<CalendarPage | null>('query_snapshot_calendar_entries', {
      query,
      epoch: await querySession(),
      requestId,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
});
export const diaryQueries = createQueryRunner<
  DiaryPage,
  { snapshot: number; offset: number; limit: number }
>('snapshot_diary', {
  run: async (_family, requestId, query) =>
    invoke<DiaryPage | null>('query_snapshot_diary', {
      query,
      epoch: await querySession(),
      requestId,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
});

export const retentionQueries = createQueryRunner<SnapshotRetentionPreview, { snapshot: number }>(
  'snapshot_retention',
  {
    run: async (_family, requestId, query) =>
      invoke<SnapshotRetentionPreview | null>('query_snapshot_retention', {
        ...query,
        epoch: await querySession(),
        requestId,
      }),
    cancel: async (family, requestId) =>
      invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
  },
);

export const historyClipQueries = createQueryRunner<HistoryClip, HistoryClipRequest>(
  'history_clip',
  {
    run: async (_family, requestId, request) =>
      invoke<HistoryClip | null>('query_history_clip', {
        request,
        epoch: await querySession(),
        requestId,
      }),
    cancel: async (family, requestId) =>
      invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
  },
);

export const snapshotCaptureQueries = createQueryRunner<SnapshotVisualFrame, { snapshot: number }>(
  'snapshot_capture',
  {
    run: async (_family, requestId, query) =>
      invoke<SnapshotVisualFrame | null>('query_snapshot_capture', {
        ...query,
        epoch: await querySession(),
        requestId,
      }),
    cancel: async (family, requestId) =>
      invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
  },
);

export const snapshotChangeQueries = createQueryRunner<
  SnapshotComparison,
  {
    before: number;
    after: number;
    offset: number;
    limit: number;
  }
>('snapshot_changes', {
  run: async (_family, requestId, query) =>
    invoke<SnapshotComparison | null>('query_snapshot_changes', {
      query,
      epoch: await querySession(),
      requestId,
    }),
  cancel: async (family, requestId) =>
    invoke<void>('cancel_query', { epoch: await querySession(), family, requestId }),
});

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
