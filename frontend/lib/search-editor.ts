import type { IndexId } from './index-context.ts';
import type { Query, SearchInspection } from './types.ts';

export interface SearchDraftRequest {
  readonly query: Query;
  readonly expression: string;
  readonly indexId: IndexId | null;
}
export type SearchDraftOutcome = SearchInspection | { kind: 'failed'; message: string };
export type SearchDraftState = { readonly request: SearchDraftRequest } & (
  { kind: 'pending' } | SearchDraftOutcome
);

export function settleSearchDraft(
  state: SearchDraftState,
  request: SearchDraftRequest,
  result: SearchDraftOutcome,
): SearchDraftState {
  return state.kind === 'pending' && state.request === request ? { request, ...result } : state;
}

export function validatedSearchDraft(
  state: SearchDraftState,
  expression: string,
  query: Query,
  indexId: IndexId | null,
): SearchDraftRequest | null {
  const request = state.request;
  return state.kind === 'valid' &&
    indexId !== null &&
    searchDraftIsCurrent(state, expression, query, indexId)
    ? request
    : null;
}

export function searchDraftIsCurrent(
  state: SearchDraftState,
  expression: string,
  query: Query,
  indexId: IndexId | null,
): boolean {
  const request = state.request;
  return (
    request.indexId === indexId && request.query === query && request.expression === expression
  );
}

export function searchErrorPosition(expression: string, offset: number): number | null {
  let bytes = 0,
    position = 1;
  const encoder = new TextEncoder();
  for (const char of expression) {
    if (bytes === offset) return position;
    bytes += encoder.encode(char).length;
    if (bytes > offset) return null;
    position += 1;
  }
  return bytes === offset ? position : null;
}
