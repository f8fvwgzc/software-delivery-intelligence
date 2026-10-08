import { useParams } from 'react-router'

import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'

import { NewProposalDialog } from '../components/new-proposal-dialog'
import { ProposalsList } from '../components/proposals-list'
import { ProposalsTableSkeleton } from '../components/proposals-table/skeleton'

/** /projects/:projectId/proposals — the same list, scoped to one project. */
export default function ProjectProposalsPage() {
  const { projectId = '' } = useParams()

  return (
    <Page>
      <PageHeader
        title="Proposals"
        description="What the platform recommends for this project, and what was decided."
        actions={<NewProposalDialog projectId={projectId} />}
      />
      <QueryBoundary fallback={<ProposalsTableSkeleton />}>
        <ProposalsList projectId={projectId} emptyAction={<NewProposalDialog projectId={projectId} />} />
      </QueryBoundary>
    </Page>
  )
}
