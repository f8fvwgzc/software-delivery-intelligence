import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader, Section } from '@/components/shared/page'

import { ApprovalHistory } from '../components/approval-history'
import { ApprovalQueue, ApprovalQueueSkeleton } from '../components/approval-queue'

export default function ApprovalsPage() {
  return (
    <Page>
      <PageHeader
        title="Approvals"
        description="Steps the policy will not run without you. Approving resumes the execution; rejecting cancels it."
      />
      <Section title="Waiting for a decision" description="Oldest first. Refreshes every few seconds.">
        <QueryBoundary fallback={<ApprovalQueueSkeleton />}>
          <ApprovalQueue />
        </QueryBoundary>
      </Section>
      <ApprovalHistory />
    </Page>
  )
}
