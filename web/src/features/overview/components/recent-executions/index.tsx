import { ActivityIcon } from 'lucide-react'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data'
import { EmptyState } from '@/components/shared/feedback'
import { cn } from '@/lib/utils'

import type { Execution } from '../../api/types'
import { DOT_CLASS, EXECUTION_STATUS } from '../../lib/labels'
import { stepStates } from '../../lib/work-queue'
import { CompactList, CompactListRow } from '../compact-list'
import { StepDots } from '../step-dots'

export function RecentExecutions({ executions }: { executions: Execution[] }) {
  return (
    <CompactList
      title="Recent executions"
      viewAll={{ to: routes.executions(), label: 'All executions' }}
      empty={
        executions.length === 0 ? (
          <EmptyState
            icon={ActivityIcon}
            title="No executions yet"
            description="Accept a proposal with an execution and its dry run shows up here."
            className="py-6"
          />
        ) : undefined
      }
    >
      {executions.map((execution) => {
        const status = EXECUTION_STATUS[execution.status]
        return (
          <CompactListRow
            key={execution.id}
            to={routes.execution(execution.id)}
            leading={
              <span className="relative flex size-1.5 shrink-0">
                {status.live ? (
                  <span
                    aria-hidden
                    className={cn(
                      'absolute inline-flex size-full animate-ping rounded-full opacity-60 motion-reduce:hidden',
                      DOT_CLASS[status.tone],
                    )}
                  />
                ) : null}
                <span aria-hidden className={cn('relative inline-flex size-1.5 rounded-full', DOT_CLASS[status.tone])} />
              </span>
            }
            title={
              <>
                <span className="font-mono text-xs text-muted-foreground">#{execution.number}</span> {execution.title}
              </>
            }
            meta={
              <>
                <StepDots states={stepStates(execution)} max={6} />
                <span className="truncate">
                  {execution.projectName} · {status.label}
                </span>
              </>
            }
            trailing={<RelativeTime value={execution.updatedAt} />}
          />
        )
      })}
    </CompactList>
  )
}
