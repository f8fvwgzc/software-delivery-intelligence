import { useSuspenseQuery } from '@tanstack/react-query'
import { CheckSquareIcon } from 'lucide-react'

import { EmptyState } from '@/components/shared/feedback'

import { approvalQueries } from '../../api/queries'
import { ApprovalRow } from './approval-row'

export { ApprovalQueueSkeleton } from './skeleton'

/** Pending approvals, oldest first, refreshed every 5s. */
export function ApprovalQueue() {
  const { data: approvals } = useSuspenseQuery(approvalQueries.pending())

  if (approvals.length === 0) {
    return (
      <EmptyState
        icon={CheckSquareIcon}
        title="Nothing waiting on you"
        description="Steps that need a human decision appear here the moment an execution reaches them."
      />
    )
  }

  const queue = [...approvals].sort((a, b) => a.requestedAt.localeCompare(b.requestedAt))

  return (
    <ol className="flex flex-col divide-y rounded-lg ring-1 ring-border" aria-label="Pending approvals">
      {queue.map((approval) => (
        <ApprovalRow key={approval.id} approval={approval} />
      ))}
    </ol>
  )
}
