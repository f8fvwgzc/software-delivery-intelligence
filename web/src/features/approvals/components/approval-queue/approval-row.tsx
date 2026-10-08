import { CheckIcon } from 'lucide-react'
import { Link } from 'react-router'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { components } from '@/lib/api/schema'

import { useDecideApproval } from '../../api/mutations'
import { POLICY_DECISION, RISK } from '../../lib/labels'
import { RejectDialog } from './reject-dialog'

interface ApprovalRowProps {
  approval: components['schemas']['Approval']
}

/** One pending request: what wants to run, why a human is needed, and the two ways out. */
export function ApprovalRow({ approval }: ApprovalRowProps) {
  const decide = useDecideApproval()
  const risk = RISK[approval.risk]
  const pendingApprove = decide.isPending && decide.variables?.approve === true
  const pendingReject = decide.isPending && decide.variables?.approve === false

  const approve = () => {
    decide.mutate(
      { approvalId: approval.id, approve: true },
      { onSuccess: () => toast.success(`Approved "${approval.stepTitle}"`) },
    )
  }

  return (
    <li className="flex flex-col gap-3 px-4 py-3 md:flex-row md:items-start md:gap-6">
      <div className="flex min-w-0 flex-1 flex-col gap-1.5">
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
          <Link to={routes.project(approval.projectId)} className="text-foreground hover:underline">
            {approval.projectName}
          </Link>
          <span aria-hidden>/</span>
          <Link to={routes.execution(approval.executionId)} className="inline-flex items-baseline gap-1 hover:underline">
            <span className="font-mono tabular-nums">#{approval.executionNumber}</span>
            <span className="truncate text-foreground">{approval.executionTitle}</span>
          </Link>
        </div>
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
          <span className="text-sm font-medium">{approval.stepTitle}</span>
          <Badge variant="secondary" className="h-5 px-1.5 font-mono text-2xs font-normal">
            {approval.actionType}
          </Badge>
          <span className="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
            <span aria-hidden className={`inline-block size-1.5 rounded-full ${risk.dot}`} />
            {risk.label}
          </span>
          <span className="text-xs text-muted-foreground">· {POLICY_DECISION[approval.policyDecision]}</span>
        </div>
        <p className="text-sm text-muted-foreground">{approval.reason}</p>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
          {approval.requiredPermissions.length > 0 && (
            <span>
              needs <span className="font-mono text-foreground/80">{approval.requiredPermissions.join(' ')}</span>
            </span>
          )}
          <span>
            requested <RelativeTime value={approval.requestedAt} className="font-mono tabular-nums" />
          </span>
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-2 md:pt-5">
        <RejectDialog approval={approval} decide={decide} />
        <Button size="sm" onClick={approve} loading={pendingApprove} disabled={pendingReject}>
          <CheckIcon data-icon="inline-start" />
          Approve
        </Button>
      </div>
    </li>
  )
}
