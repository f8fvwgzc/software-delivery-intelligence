import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'

import { NewProposalDialog } from '../components/new-proposal-dialog'
import { ProposalsList } from '../components/proposals-list'
import { ProposalsTableSkeleton } from '../components/proposals-table/skeleton'

/** /proposals — every proposal across projects. */
export default function ProposalsPage() {
  return (
    <Page>
      <PageHeader
        title="Proposals"
        description="Ranked, evidence-backed options the platform has proposed. Negotiate, then accept one into an execution."
        actions={<NewProposalDialog />}
      />
      <Panel title="All proposals" padding="none" skeleton={<ProposalsTableSkeleton />}>
        <ProposalsList emptyAction={<NewProposalDialog />} />
      </Panel>
    </Page>
  )
}
