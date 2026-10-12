import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { Person, RelativeTime } from '@/components/shared/data'
import { Badge } from '@/components/ui/badge'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { DECISION_SOURCE_LABEL, DECISION_STATUS } from '../../lib/labels'
import { SupersedeDialog } from '../supersede-dialog'

type Decision = components['schemas']['Decision']

interface DecisionItemProps {
  decision: Decision
  projectName?: string
}

export function DecisionItem({ decision, projectName }: DecisionItemProps) {
  const status = DECISION_STATUS[decision.status]
  const active = decision.status === 'active'

  return (
    <li className={cn('flex flex-col gap-2 px-4 py-3', !active && 'text-muted-foreground')}>
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <div className="flex items-center gap-2">
            <span
              className={cn('size-1.5 shrink-0 rounded-full', active ? 'bg-success' : 'bg-muted-foreground/60')}
              aria-hidden="true"
            />
            <h3 className={cn('truncate text-sm font-medium', !active && 'line-through decoration-border')}>
              {decision.title}
            </h3>
            <span className="sr-only">{status.label}</span>
          </div>
          <p className="text-sm">{decision.statement}</p>
        </div>
        {active ? <SupersedeDialog decision={decision} /> : null}
      </div>

      {decision.reasons.length > 0 ? (
        <ul className="flex list-disc flex-col gap-0.5 pl-5 text-xs text-muted-foreground">
          {decision.reasons.map((reason, index) => (
            <li key={`${index}-${reason}`}>{reason}</li>
          ))}
        </ul>
      ) : null}

      <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-2xs text-muted-foreground">
        {decision.tags.length > 0 ? (
          <ul className="flex flex-wrap gap-1" aria-label="Tags">
            {decision.tags.map((tag) => (
              <li key={tag}>
                <Badge variant="secondary" className="h-5 px-1.5 font-mono text-2xs font-normal">
                  {tag}
                </Badge>
              </li>
            ))}
          </ul>
        ) : null}
        <span>
          Approved by <Person value={decision.approvedBy} fallback="nobody yet" avatar={false} />
        </span>
        <span>
          Recorded <RelativeTime value={decision.createdAt} className="font-mono" />
        </span>
        <span>{DECISION_SOURCE_LABEL[decision.source]}</span>
        {decision.projectId ? (
          <Link to={routes.project(decision.projectId)} className="hover:underline">
            {projectName ?? decision.projectId}
          </Link>
        ) : null}
        {decision.proposalId ? (
          <Link to={routes.proposal(decision.proposalId)} className="hover:underline">
            Proposal
          </Link>
        ) : null}
        {decision.supersededAt ? (
          <span>
            Superseded <RelativeTime value={decision.supersededAt} className="font-mono" />
          </span>
        ) : null}
      </div>
    </li>
  )
}
