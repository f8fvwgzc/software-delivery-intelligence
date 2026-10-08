import { PlugIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { EmptyState } from '@/components/shared/feedback'
import { Button } from '@/components/ui/button'

/** The whole overview when nothing is connected: one thing to do. */
export function ConnectCta() {
  return (
    <EmptyState
      icon={PlugIcon}
      title="No repositories connected"
      description="Connect one and the platform scans it, records what it finds, and proposes the next delivery step with evidence."
      action={
        <Button asChild>
          <Link to={routes.connect()}>
            <PlugIcon data-icon="inline-start" />
            Connect a repository
          </Link>
        </Button>
      }
      className="py-16"
    />
  )
}
