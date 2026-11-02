import type { SearchDraftRequest } from './search-editor.ts';
import type { SearchTranslation } from './types.ts';

export interface TranslationRequest extends Omit<SearchDraftRequest, 'expression'> {
  readonly source: string;
  readonly draft: string;
}
export type TranslationState = { readonly request: TranslationRequest } & (
  | { kind: 'pending' }
  | { kind: 'ready'; proposal: SearchTranslation }
  | { kind: 'failed'; message: string }
);

export function translationIsCurrent(
  state: TranslationState | null,
  context: TranslationRequest,
): boolean {
  return (
    state !== null &&
    state.request.source === context.source &&
    state.request.draft === context.draft &&
    state.request.query === context.query &&
    state.request.indexId === context.indexId
  );
}

export function settleTranslation(
  state: TranslationState | null,
  request: TranslationRequest,
  outcome: { kind: 'ready'; proposal: SearchTranslation } | { kind: 'failed'; message: string },
): TranslationState | null {
  return state?.kind === 'pending' && state.request === request ? { request, ...outcome } : state;
}

export function translationDraft(
  state: TranslationState | null,
  context: TranslationRequest,
  partialAccepted: boolean,
): string | null {
  if (
    state?.kind !== 'ready' ||
    !translationIsCurrent(state, context) ||
    context.indexId === null ||
    !state.proposal.expression.trim() ||
    (state.proposal.unparsed.length > 0 && !partialAccepted)
  )
    return null;
  return state.proposal.expression;
}
