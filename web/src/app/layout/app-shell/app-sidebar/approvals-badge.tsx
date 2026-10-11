import { useQuery } from '@tanstack/react-query'

import { SidebarMenuBadge } from '@/components/ui/sidebar'
import { approvalQueries } from '@/features/approvals/api/queries'
import { fallbackPoll } from '@/lib/live/connection-store'

const COUNT_INTERVAL = 15_000

/** Pending-approval count next to the Approvals nav item; hidden when there is nothing to do. */
export function ApprovalsBadge() {
  const { data: count } = useQuery({
    ...approvalQueries.list({ status: 'pending' }),
    refetchInterval: () => fallbackPoll(COUNT_INTERVAL),
    select: (approvals) => approvals.length,
  })

  if (!count) return null

  return (
    <SidebarMenuBadge className="bg-warning/10 font-medium tabular-nums text-warning" aria-label={`${count} pending approvals`}>
      {count > 99 ? '99+' : count}
    </SidebarMenuBadge>
  )
}
