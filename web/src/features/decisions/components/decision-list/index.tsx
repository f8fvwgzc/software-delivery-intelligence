import { ScaleIcon } from 'lucide-react'
import type { ReactNode } from 'react'

import { EmptyState } from '@/components/shared/feedback'
import type { components } from '@/lib/api/schema'

import { DECISION_SCOPE_DESCRIPTION, DECISION_SCOPE_LABEL, groupByScope } from '../../lib/labels'
import { DecisionItem } from './decision-item'

type Decision = components['schemas']['Decision']

interface DecisionListProps {
  decisions: Decision[]
  /** Project names by id, for project-scoped decisions. */
  projectNames: Map<string, string>
  emptyAction?: ReactNode
}

/** Decisions grouped by scope, widest first. */
export function DecisionList({ decisions, projectNames, emptyAction }: DecisionListProps) {
  if (decisions.length === 0) {
    return (
      <EmptyState
        icon={ScaleIcon}
        title="No decisions recorded"
        description="Decisions like “we standardise on Azure” steer every ranking. Record one, or accept a proposal."
        action={emptyAction}
      />
    )
  }

  const groups = groupByScope(decisions)

  return (
    <div className="flex flex-col gap-6">
      {groups.map((group) => (
        <section key={group.scope} className="flex flex-col gap-2" aria-labelledby={`scope-${group.scope}`}>
          <div className="flex items-baseline gap-2">
            <h2 id={`scope-${group.scope}`} className="text-sm font-medium">
              {DECISION_SCOPE_LABEL[group.scope]}
            </h2>
            <span className="text-xs tabular-nums text-muted-foreground">{group.items.length}</span>
            <span className="hidden text-xs text-muted-foreground sm:inline">
              {DECISION_SCOPE_DESCRIPTION[group.scope]}
            </span>
          </div>
          <ol className="flex flex-col divide-y divide-border/60 rounded-lg bg-card ring-1 ring-border">
            {group.items.map((decision) => (
              <DecisionItem
                key={decision.id}
                decision={decision}
                projectName={decision.projectId ? projectNames.get(decision.projectId) : undefined}
              />
            ))}
          </ol>
        </section>
      ))}
    </div>
  )
}
