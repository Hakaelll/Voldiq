export const defaultReviewWeights = {
  size: 25,
  age: 15,
  regeneration: 20,
  duplication: 15,
  provenance: 15,
  recovery: 10,
};

export type ReviewWeights = typeof defaultReviewWeights;

export const reviewFactors = [
  ['size', 'Size'],
  ['age', 'Time since modification'],
  ['regeneration', 'Regeneration'],
  ['duplication', 'Exact duplication'],
  ['provenance', 'Rule match'],
  ['recovery', 'Local copy evidence'],
] as const satisfies ReadonlyArray<readonly [keyof ReviewWeights, string]>;

export const reviewEvidence = {
  indexed_logical: 'Indexed logical size',
  indexed_modified: 'Indexed modification date',
  browser_http_payload: 'Supported browser cache may be downloaded again',
  scoped_recipe: 'Matches this rule within the indexed scope',
  exact_hash: 'Matching content in the current duplicate analysis',
  independent_local_copy: 'Separate local file with matching content',
  unavailable: 'Evidence unavailable',
};

export interface ReviewPriority {
  points: number;
  coverage: number;
  signals: {
    evidence: keyof typeof reviewEvidence;
    value: number | null;
    weight: number;
    points: number;
  }[];
}

function boundedInteger(value: unknown, maximum: number): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= maximum;
}

function record(value: unknown): Record<string, unknown> | null {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
}

export function readReviewWeights(value: unknown): ReviewWeights | null {
  const source = record(value);
  if (!source || Object.keys(source).length !== reviewFactors.length) return null;
  const result = { ...defaultReviewWeights };
  for (const [key] of reviewFactors) {
    const weight = source[key];
    if (!boundedInteger(weight, 100)) return null;
    result[key] = weight;
  }
  return Object.values(result).some((weight) => weight > 0) ? result : null;
}

function isEvidence(value: unknown): value is keyof typeof reviewEvidence {
  return typeof value === 'string' && Object.hasOwn(reviewEvidence, value);
}

export function readReviewPriority(value: unknown): ReviewPriority {
  const result = record(value);
  const invalid = () => {
    throw Error('Invalid cleanup priority evidence.');
  };
  if (
    !result ||
    !boundedInteger(result.points, 10_000) ||
    !boundedInteger(result.coverage, 100) ||
    !Array.isArray(result.signals) ||
    result.signals.length !== reviewFactors.length
  )
    return invalid();
  const signals = result.signals.map((value: unknown) => {
    const signal = record(value);
    if (
      !signal ||
      !isEvidence(signal.evidence) ||
      (signal.value !== null && !boundedInteger(signal.value, 100)) ||
      !boundedInteger(signal.weight, 100) ||
      !boundedInteger(signal.points, 10_000) ||
      (signal.value === null) !== (signal.evidence === 'unavailable')
    )
      return invalid();
    return {
      evidence: signal.evidence,
      value: signal.value,
      weight: signal.weight,
      points: signal.points,
    };
  });
  return { points: result.points, coverage: result.coverage, signals };
}

const pointsFormat = new Intl.NumberFormat(undefined, { maximumFractionDigits: 2 });
export function priorityPoints(points: number): string {
  return pointsFormat.format(points / 100);
}
