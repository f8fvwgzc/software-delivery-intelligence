import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'

import { ActivityFeed } from '../components/activity-feed'

/** /activity — the audit of the review loop, newest first. */
export default function ActivityPage() {
  return (
    <Page>
      <PageHeader
        title="Activity"
        description="Who started, decided and published what, across every review in this workspace."
      />
      <Panel title="Review activity" padding="none" skeletonRows={6}>
        <ActivityFeed />
      </Panel>
    </Page>
  )
}
