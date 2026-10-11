import { useParams, useSearchParams } from 'react-router'

import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'

import { NewProposalDialog } from '../components/new-proposal-dialog'
import { ProposalsList } from '../components/proposals-list'
import { ProposalsTableSkeleton } from '../components/proposals-table/skeleton'
import { isProposalKind } from '../lib/labels'

/** /projects/:projectId/proposals — the same list, scoped to one project. `?kind=` preselects the dialog. */
export default function ProjectProposalsPage() {
  const { projectId = '' } = useParams()
  const [searchParams] = useSearchParams()
  const requestedKind = searchParams.get('kind')
  const initialKind = isProposalKind(requestedKind) ? requestedKind : undefined

  return (
    <Page>
      <PageHeader
        title="Proposals"
        description="What the platform recommends for this project, and what was decided."
        actions={<NewProposalDialog projectId={projectId} initialKind={initialKind} />}
      />
      <QueryBoundary fallback={<ProposalsTableSkeleton />}>
        <ProposalsList
          projectId={projectId}
          emptyAction={<NewProposalDialog projectId={projectId} initialKind={initialKind} />}
        />
      </QueryBoundary>
    </Page>
  )
}
