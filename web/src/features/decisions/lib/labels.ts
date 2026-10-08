import type { StatusTone } from '@/components/shared/data'
import type { components } from '@/lib/api/schema'

export type DecisionScope = components['schemas']['DecisionScope']
export type DecisionSource = components['schemas']['DecisionSource']
export type DecisionStatus = components['schemas']['DecisionStatus']

/** Widest scope first: that is how a reader wants to see the rules. */
export const DECISION_SCOPES = ['organization', 'department', 'team', 'project'] as const satisfies readonly DecisionScope[]

export const DECISION_SCOPE_LABEL: Record<DecisionScope, string> = {
  unspecified: 'Unscoped',
  project: 'Project',
  team: 'Team',
  department: 'Department',
  organization: 'Organization',
}

export const DECISION_SCOPE_DESCRIPTION: Record<DecisionScope, string> = {
  unspecified: 'Decisions without a scope.',
  project: 'Applies to one connected repository.',
  team: 'Applies to every project the team owns.',
  department: 'Applies across teams.',
  organization: 'Applies everywhere.',
}

export const DECISION_STATUS: Record<DecisionStatus, { label: string; tone: StatusTone }> = {
  unspecified: { label: 'Unknown', tone: 'neutral' },
  active: { label: 'Active', tone: 'success' },
  superseded: { label: 'Superseded', tone: 'neutral' },
}

export const DECISION_SOURCE_LABEL: Record<DecisionSource, string> = {
  unspecified: 'Unknown source',
  manual: 'Recorded by hand',
  negotiation: 'From negotiation',
  proposal_accepted: 'From an accepted proposal',
  question_answered: 'From an answered question',
}

/** Groups decisions by scope in display order; empty scopes are skipped. */
export function groupByScope<T extends { scope: DecisionScope }>(decisions: T[]) {
  const order: DecisionScope[] = [...DECISION_SCOPES, 'unspecified']
  const buckets = new Map<DecisionScope, T[]>()
  for (const decision of decisions) {
    const bucket = buckets.get(decision.scope)
    if (bucket) bucket.push(decision)
    else buckets.set(decision.scope, [decision])
  }
  return order.flatMap((scope) => {
    const items = buckets.get(scope)
    return items ? [{ scope, items }] : []
  })
}
