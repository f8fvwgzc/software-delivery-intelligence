import { ArrowRightIcon, CheckCircle2Icon, CircleSlashIcon, HistoryIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { Alert, AlertAction, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import type { components } from '@/lib/api/schema'

type Proposal = components['schemas']['Proposal']

/** Shown above a closed proposal; everything below it is read-only. */
export function ProposalStatusBanner({ proposal }: { proposal: Proposal }) {
  if (proposal.status === 'accepted') {
    const accepted = proposal.options.find((option) => option.id === proposal.acceptedOptionId)
    return (
      <Alert>
        <CheckCircle2Icon className="text-success" />
        <AlertTitle>Accepted{accepted ? `: ${accepted.title}` : ''}</AlertTitle>
        <AlertDescription>
          {proposal.executionId
            ? 'An execution was created from this plan. The proposal is now read-only.'
            : 'No execution was created. Start one from the executions page when ready.'}
        </AlertDescription>
        {proposal.executionId ? (
          <AlertAction>
            <Button asChild size="xs" variant="outline">
              <Link to={routes.execution(proposal.executionId)}>
                Open execution
                <ArrowRightIcon data-icon="inline-end" />
              </Link>
            </Button>
          </AlertAction>
        ) : null}
      </Alert>
    )
  }

  if (proposal.status === 'rejected') {
    return (
      <Alert>
        <CircleSlashIcon className="text-destructive" />
        <AlertTitle>Rejected</AlertTitle>
        <AlertDescription>This proposal was rejected and is read-only. Generate a new one to try again.</AlertDescription>
      </Alert>
    )
  }

  if (proposal.status === 'superseded') {
    return (
      <Alert>
        <HistoryIcon className="text-muted-foreground" />
        <AlertTitle>Superseded</AlertTitle>
        <AlertDescription>A newer proposal replaced this one. It is kept for reference.</AlertDescription>
      </Alert>
    )
  }

  return null
}
