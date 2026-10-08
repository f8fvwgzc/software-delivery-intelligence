import { useSuspenseQuery } from '@tanstack/react-query'
import { ChevronRightIcon, HistoryIcon } from 'lucide-react'
import { useState } from 'react'

import { QueryBoundary } from '@/components/shared/feedback'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'

import { approvalQueries } from '../../api/queries'
import { HistoryTable, HistoryTableSkeleton } from './history-table'

/** Decided and expired approvals, collapsed by default and loaded on open. */
export function ApprovalHistory() {
  const [open, setOpen] = useState(false)

  return (
    <Collapsible open={open} onOpenChange={setOpen} className="flex flex-col gap-3">
      <CollapsibleTrigger className="group/trigger inline-flex w-fit items-center gap-1.5 rounded-md text-sm font-medium outline-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50">
        <ChevronRightIcon className="size-4 text-muted-foreground transition-transform duration-150 group-data-[state=open]/trigger:rotate-90" />
        <HistoryIcon className="size-4 text-muted-foreground" />
        History
        <span className="text-xs font-normal text-muted-foreground">approved, rejected and expired</span>
      </CollapsibleTrigger>
      <CollapsibleContent>
        <QueryBoundary fallback={<HistoryTableSkeleton />}>
          <HistoryContent />
        </QueryBoundary>
      </CollapsibleContent>
    </Collapsible>
  )
}

function HistoryContent() {
  const { data: history } = useSuspenseQuery(approvalQueries.history())
  return <HistoryTable approvals={history} />
}
