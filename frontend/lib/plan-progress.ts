export type PlanProgress =
  | { kind: 'recycling'; processed: number }
  | {
      kind: 'copy';
      node: number;
      stage: 'copying' | 'readback' | 'source_recheck';
      bytes: string;
      total: string;
    };

function byteCount(value: unknown): value is string {
  return (
    typeof value === 'string' &&
    /^(0|[1-9]\d{0,19})$/.test(value) &&
    BigInt(value) <= 18446744073709551615n
  );
}

export function readPlanProgress(value: unknown): PlanProgress | null {
  if (!value || typeof value !== 'object' || !('kind' in value)) return null;
  if (
    value.kind === 'recycling' &&
    'processed' in value &&
    typeof value.processed === 'number' &&
    Number.isSafeInteger(value.processed) &&
    value.processed >= 0 &&
    value.processed <= 500
  ) {
    return { kind: 'recycling', processed: value.processed };
  }
  if (
    value.kind === 'copy' &&
    'node' in value &&
    typeof value.node === 'number' &&
    Number.isSafeInteger(value.node) &&
    value.node >= 0 &&
    value.node <= 4294967295 &&
    'stage' in value &&
    (value.stage === 'copying' || value.stage === 'readback' || value.stage === 'source_recheck') &&
    'bytes' in value &&
    byteCount(value.bytes) &&
    'total' in value &&
    byteCount(value.total) &&
    BigInt(value.bytes) <= BigInt(value.total)
  ) {
    return {
      kind: 'copy',
      node: value.node,
      stage: value.stage,
      bytes: value.bytes,
      total: value.total,
    };
  }
  return null;
}
