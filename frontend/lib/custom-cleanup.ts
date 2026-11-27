import type { ReviewPriority, ReviewWeights } from './review-priority';
import type { NodeId } from './types';

export interface CustomRuleReference {
  id: number;
  revision: number;
}

export interface CustomRuleDraft {
  name: string;
  extension: string;
  minimum_age_days: number;
  minimum_logical: string;
  expression: string;
}

export interface CustomRule extends CustomRuleDraft {
  reference: CustomRuleReference;
  root: string;
}

export interface CustomRuleQuery {
  offset: number;
  limit: number;
  weights: ReviewWeights;
}

export interface CustomRulePage {
  rule: CustomRule;
  complete: boolean;
  unverified_branches: number;
  unverified_files: number;
  assessed_at: number;
  total: number;
  logical: string;
  items: {
    id: NodeId;
    path: string;
    logical: string;
    allocated: string | null;
    hard_link: boolean;
    modified: number | null;
    priority: ReviewPriority;
  }[];
}
