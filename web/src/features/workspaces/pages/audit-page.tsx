import { useSuspenseInfiniteQuery } from '@tanstack/react-query'
import { useDeferredValue, useState } from 'react'

import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader, Section } from '@/components/shared/page'
import { useAuth } from '@/features/auth/lib/auth-context'

import { workspaceQueries } from '../api/queries'
import { AuditFilter } from '../components/audit-filter'
import { AuditTable, AuditTableSkeleton } from '../components/audit-table'

interface AuditPanelProps {
  actionPrefix: string
  onClearFilter: () => void
}

function AuditPanel({ actionPrefix, onClearFilter }: AuditPanelProps) {
  const { currentWorkspace } = useAuth()
  const query = useSuspenseInfiniteQuery(workspaceQueries.audit(currentWorkspace.id, actionPrefix))
  const entries = query.data.pages.flat()

  return (
    <AuditTable
      entries={entries}
      actionPrefix={actionPrefix}
      hasEarlier={query.hasNextPage}
      loadingEarlier={query.isFetchingNextPage}
      onLoadEarlier={() => query.fetchNextPage()}
      onClearFilter={onClearFilter}
    />
  )
}

export default function AuditPage() {
  const [filter, setFilter] = useState('')
  const actionPrefix = useDeferredValue(filter.trim())

  return (
    <Page>
      <PageHeader
        title="Audit"
        description="Every action that changed this workspace, newest first: who, what, on which entity, and the request it came in on."
      />
      <Section title="Entries" description="Filter by the action's family, or type any prefix.">
        <AuditFilter value={filter} onChange={setFilter} />
        <QueryBoundary fallback={<AuditTableSkeleton />}>
          <AuditPanel actionPrefix={actionPrefix} onClearFilter={() => setFilter('')} />
        </QueryBoundary>
      </Section>
    </Page>
  )
}
