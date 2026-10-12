import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'

import { Identities, IdentitiesSkeleton } from '../components/identities'
import { PoliciesTable, PoliciesTableSkeleton } from '../components/policies-table'

export default function PoliciesPage() {
  return (
    <Page>
      <PageHeader
        title="Policies"
        description="What each action type may do before a human is asked. Defaults marked as required can be tightened elsewhere but never relaxed here."
      />
      <Panel
        title="Action policies"
        description="Every step is checked against its action type right before it runs."
        padding="none"
        skeleton={<PoliciesTableSkeleton />}
      >
        <PoliciesTable />
      </Panel>
      <Panel
        title="Identities"
        description="Who executions run as, and which permissions they hold. A missing grant stops the step before it starts."
        skeleton={<IdentitiesSkeleton />}
      >
        <Identities />
      </Panel>
    </Page>
  )
}
