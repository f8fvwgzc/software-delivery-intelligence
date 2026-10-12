import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'

import { ApprovalHistory } from '../components/approval-history'
import { ApprovalQueue, ApprovalQueueSkeleton } from '../components/approval-queue'

export default function ApprovalsPage() {
  return (
    <Page>
      <PageHeader
        title="Approvals"
        description="Steps the policy will not run without you. Approving resumes the execution; rejecting cancels it."
      />
      <Panel title="Waiting for a decision" description="Oldest first; live." padding="none" skeleton={<ApprovalQueueSkeleton />}>
        <ApprovalQueue />
      </Panel>
      <ApprovalHistory />
    </Page>
  )
}
