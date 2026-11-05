export type CopiedResult = {
  files: number;
  verifiedBytes: string;
  originals: 'kept' | 'unchanged' | 'review_recycling';
  publication: 'NotAttempted' | 'Uncertain' | 'Confirmed' | null;
  privateCleanupFailed: boolean;
  requiresReview: boolean;
};

export type PlanResult =
  | { kind: 'unavailable' }
  | { kind: 'copy'; copies: CopiedResult }
  | {
      kind: 'execution';
      copies: CopiedResult | null;
      recycled: number;
      returned: number;
      unmoved: number;
      notStarted: number;
      unknown: number;
      requiresReview: boolean;
    };

function readCopies(value: unknown): CopiedResult | null {
  if (
    !value ||
    typeof value !== 'object' ||
    !('phase' in value) ||
    !('copied_files' in value) ||
    typeof value.copied_files !== 'number' ||
    !Number.isSafeInteger(value.copied_files) ||
    value.copied_files < 0 ||
    value.copied_files > 500 ||
    !('verified_bytes' in value) ||
    typeof value.verified_bytes !== 'string' ||
    !/^(0|[1-9]\d{0,38})$/.test(value.verified_bytes) ||
    BigInt(value.verified_bytes) > (1n << 128n) - 1n ||
    (value.copied_files === 0 && value.verified_bytes !== '0') ||
    !('full_restore_verified' in value) ||
    value.full_restore_verified !== false ||
    !('immediate_free_bytes' in value) ||
    value.immediate_free_bytes !== '0'
  )
    return null;
  const copies: CopiedResult = {
    files: value.copied_files,
    verifiedBytes: value.verified_bytes,
    originals: 'review_recycling',
    publication: null,
    privateCleanupFailed: false,
    requiresReview: false,
  };
  if (
    value.phase === 'copy_completed' &&
    value.copied_files > 0 &&
    'originals' in value &&
    value.originals === 'kept'
  )
    return { ...copies, originals: 'kept' };
  if (
    value.phase === 'copy_stopped' &&
    'originals' in value &&
    value.originals === 'unchanged' &&
    'publication' in value &&
    (value.publication === 'NotAttempted' ||
      value.publication === 'Uncertain' ||
      value.publication === 'Confirmed') &&
    'private_cleanup_failed' in value &&
    typeof value.private_cleanup_failed === 'boolean'
  )
    return {
      ...copies,
      originals: 'unchanged',
      publication: value.publication,
      privateCleanupFailed: value.private_cleanup_failed,
      requiresReview: true,
    };
  if (
    (value.phase === 'execution' || value.phase === 'original_preflight') &&
    'catalogue_requires_review' in value &&
    typeof value.catalogue_requires_review === 'boolean' &&
    'original_phase_failed' in value &&
    typeof value.original_phase_failed === 'boolean'
  )
    return {
      ...copies,
      requiresReview:
        value.catalogue_requires_review ||
        value.original_phase_failed ||
        value.phase === 'original_preflight',
    };
  return null;
}

export function readPlanResult(value: unknown): PlanResult {
  const unavailable: PlanResult = { kind: 'unavailable' };
  if (
    value &&
    typeof value === 'object' &&
    'phase' in value &&
    (value.phase === 'copy_completed' ||
      value.phase === 'copy_stopped' ||
      value.phase === 'original_preflight')
  ) {
    const copies = readCopies(value);
    return copies ? { kind: 'copy', copies } : unavailable;
  }
  if (
    !value ||
    typeof value !== 'object' ||
    !('phase' in value) ||
    value.phase !== 'execution' ||
    !('items' in value) ||
    !Array.isArray(value.items) ||
    value.items.length < 1 ||
    value.items.length > 500 ||
    !('total' in value) ||
    value.total !== value.items.length ||
    !('journal_failed' in value) ||
    typeof value.journal_failed !== 'boolean' ||
    !('cleanup_requires_review' in value) ||
    typeof value.cleanup_requires_review !== 'boolean'
  )
    return unavailable;
  const result: Extract<PlanResult, { kind: 'execution' }> = {
    kind: 'execution',
    copies: 'copied_files' in value ? readCopies(value) : null,
    recycled: 0,
    returned: 0,
    unmoved: 0,
    notStarted: 0,
    unknown: 0,
    requiresReview: value.journal_failed || value.cleanup_requires_review,
  };
  if ('copied_files' in value && !result.copies) return unavailable;
  if (result.copies?.requiresReview) result.requiresReview = true;
  const items: readonly unknown[] = value.items;
  for (const item of items) {
    if (!item || typeof item !== 'object' || !('outcome' in item)) return unavailable;
    switch (item.outcome) {
      case 'recycled':
        result.recycled++;
        break;
      case 'returned':
        result.returned++;
        break;
      case 'unmoved':
        result.unmoved++;
        break;
      case 'not_started':
        result.notStarted++;
        break;
      case 'unknown':
        result.unknown++;
        result.requiresReview = true;
        break;
      default:
        return unavailable;
    }
  }
  return result;
}
