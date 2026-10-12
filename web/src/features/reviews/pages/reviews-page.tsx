import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'

import { ReviewsList } from '../components/reviews-list'

/** /reviews — every review across projects. */
export default function ReviewsPage() {
  return (
    <Page>
      <PageHeader
        title="Reviews"
        description="Pull request reviews the platform prepared. Each one waits for your decisions before anything reaches GitHub."
      />
      <Panel title="All reviews" padding="none">
        <ReviewsList />
      </Panel>
    </Page>
  )
}
