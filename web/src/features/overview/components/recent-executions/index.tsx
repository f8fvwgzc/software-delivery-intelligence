import { ActivityIcon } from 'lucide-react'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data'
import { PanelEmpty } from '@/components/system/panel'
import { TagDot } from '@/components/system/tag'

import type { Execution } from '../../api/types'
import { EXECUTION_STATUS } from '../../lib/labels'
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
          <PanelEmpty
            icon={ActivityIcon}
            title="No executions yet"
            description="Accept a proposal with an execution and its dry run shows up here."
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
            leading={<TagDot tone={status.tone} live={status.live} />}
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
            trailing={<RelativeTime value={execution.updatedAt} className="font-mono tabular-nums" />}
          />
        )
      })}
    </CompactList>
  )
}
