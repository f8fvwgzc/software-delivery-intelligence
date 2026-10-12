import { useSuspenseInfiniteQuery } from '@tanstack/react-query'
import { useDeferredValue, useState } from 'react'

import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'
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
      <AuditFilter value={filter} onChange={setFilter} />
      <Panel title="Entries" description="Newest first." padding="none" skeleton={<AuditTableSkeleton />}>
        <AuditPanel actionPrefix={actionPrefix} onClearFilter={() => setFilter('')} />
      </Panel>
    </Page>
  )
}
