import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader, Section } from '@/components/shared/page'

import { Identities, IdentitiesSkeleton } from '../components/identities'
import { PoliciesTable, PoliciesTableSkeleton } from '../components/policies-table'

export default function PoliciesPage() {
  return (
    <Page>
      <PageHeader
        title="Policies"
        description="What each action type may do before a human is asked. Defaults marked as required can be tightened elsewhere but never relaxed here."
      />
      <Section
        title="Action policies"
        description="Every step is checked against its action type right before it runs."
      >
        <QueryBoundary fallback={<PoliciesTableSkeleton />}>
          <PoliciesTable />
        </QueryBoundary>
      </Section>
      <Section
        title="Identities"
        description="Who executions run as, and which permissions they currently hold. A missing grant stops the step before it starts."
      >
        <QueryBoundary fallback={<IdentitiesSkeleton />}>
          <Identities />
        </QueryBoundary>
      </Section>
    </Page>
  )
}
