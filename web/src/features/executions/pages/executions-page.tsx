import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'

import { ExecutionsTable, ExecutionsTableSkeleton } from '../components/executions-table'
import { NewDryRunDialog } from '../components/new-dry-run-dialog'

export default function ExecutionsPage() {
  return (
    <Page>
      <PageHeader
        title="Executions"
        description="Every plan run one policy-gated step at a time. Live rows refresh on their own."
        actions={<NewDryRunDialog />}
      />
      <QueryBoundary fallback={<ExecutionsTableSkeleton />}>
        <ExecutionsTable emptyAction={<NewDryRunDialog variant="outline" size="sm" />} />
      </QueryBoundary>
    </Page>
  )
}
