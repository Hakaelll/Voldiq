import type { CloudFilter, CloudState, Entry } from './types.ts';

export const CLOUD_FILTERS: { value: CloudFilter; label: string }[] = [
  { value: 'placeholder', label: 'Placeholders' },
  { value: 'recall_required', label: 'Recall flag observed' },
  { value: 'no_recall_flag', label: 'No recall flag observed' },
  { value: 'not_placeholder', label: 'Not a detected placeholder' },
  { value: 'unknown', label: 'Unknown state' },
];

export function parseCloudFilter(value: string): CloudFilter | null | undefined {
  return value === '' ? null : CLOUD_FILTERS.find((filter) => filter.value === value)?.value;
}

export function cloudLabel(state: CloudState): string {
  switch (state) {
    case 'unknown':
      return 'Unknown';
    case 'not_placeholder':
      return 'Not a detected placeholder';
    case 'file_no_recall_flag':
      return 'File placeholder · no recall flag';
    case 'file_recall_required':
      return 'File placeholder · recall required';
    case 'directory_no_recall_flag':
      return 'Folder placeholder · no recall flag';
    case 'directory_recall_required':
      return 'Folder placeholder · recall required';
    default: {
      const exhaustive: never = state;
      return exhaustive;
    }
  }
}

export function entryKindLabel(entry: Pick<Entry, 'kind' | 'cloud' | 'hard_link'>): string {
  switch (entry.cloud) {
    case 'file_no_recall_flag':
    case 'file_recall_required':
      return 'File placeholder';
    case 'directory_no_recall_flag':
    case 'directory_recall_required':
      return 'Folder placeholder';
    case 'unknown':
    case 'not_placeholder':
      return entry.kind === 'directory'
        ? 'Folder'
        : entry.kind === 'link'
          ? 'Reparse point'
          : entry.hard_link
            ? 'File · hard link'
            : 'File';
    default: {
      const exhaustive: never = entry.cloud;
      return exhaustive;
    }
  }
}
