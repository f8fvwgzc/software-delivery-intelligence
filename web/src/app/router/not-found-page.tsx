import { CompassIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { EmptyState } from '@/components/shared/feedback/empty-state'
import { Page } from '@/components/shared/page'
import { Button } from '@/components/ui/button'

export function NotFoundPage() {
  return (
    <Page>
      <EmptyState
        icon={CompassIcon}
        title="This page does not exist"
        description="The link may be out of date, or the item was removed."
        action={
          <Button asChild variant="outline">
            <Link to={routes.overview()}>Back to overview</Link>
          </Button>
        }
      />
    </Page>
  )
}
