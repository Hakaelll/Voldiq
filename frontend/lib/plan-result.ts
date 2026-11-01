export type PlanResult =
  | { kind: 'unavailable' }
  | {
      kind: 'execution';
      recycled: number;
      returned: number;
      unmoved: number;
      notStarted: number;
      unknown: number;
      requiresReview: boolean;
    };

export function readPlanResult(value: unknown): PlanResult {
  const unavailable: PlanResult = { kind: 'unavailable' };
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
    recycled: 0,
    returned: 0,
    unmoved: 0,
    notStarted: 0,
    unknown: 0,
    requiresReview: value.journal_failed || value.cleanup_requires_review,
  };
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
