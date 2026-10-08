import { useQueryClient } from '@tanstack/react-query'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime, ScoreBar } from '@/components/shared/data'
import { TableCell, TableRow } from '@/components/ui/table'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { proposalQuery } from '../../api/queries'
import { PROPOSAL_KIND_LABEL, PROPOSAL_STATUS } from '../../lib/labels'

type Proposal = components['schemas']['Proposal']

const DOT_CLASS = {
  neutral: 'bg-muted-foreground/60',
  info: 'bg-info',
  success: 'bg-success',
  warning: 'bg-warning',
  destructive: 'bg-destructive',
  brand: 'bg-brand',
} as const

interface ProposalRowProps {
  proposal: Proposal
  projectName?: string
}

export function ProposalRow({ proposal, projectName }: ProposalRowProps) {
  const queryClient = useQueryClient()
  const status = PROPOSAL_STATUS[proposal.status]
  const recommended = proposal.options.find((option) => option.id === proposal.recommendedOptionId)

  // Warm the detail cache on intent so the proposal page paints at once.
  const prefetch = () => queryClient.prefetchQuery(proposalQuery(proposal.id))

  return (
    <TableRow className="h-11">
      <TableCell className="px-3 text-xs text-muted-foreground">{PROPOSAL_KIND_LABEL[proposal.kind]}</TableCell>
      <TableCell className="max-w-0 px-3">
        <Link
          to={routes.proposal(proposal.id)}
          onMouseEnter={prefetch}
          onFocus={prefetch}
          className="block truncate font-medium outline-none hover:underline focus-visible:underline"
        >
          {proposal.title}
        </Link>
      </TableCell>
      <TableCell className="px-3">
        <span className="inline-flex items-center gap-1.5 text-xs">
          <span className={cn('size-1.5 shrink-0 rounded-full', DOT_CLASS[status.tone])} />
          {status.label}
        </span>
      </TableCell>
      <TableCell className="px-3">
        {recommended ? (
          <div className="flex min-w-0 flex-col gap-1">
            <span className="truncate text-xs">{recommended.title}</span>
            <ScoreBar value={recommended.score?.overall ?? 0} className="max-w-56" />
          </div>
        ) : (
          <span className="text-xs text-muted-foreground">No option</span>
        )}
      </TableCell>
      <TableCell className="px-3 text-right font-mono text-xs tabular-nums text-muted-foreground">
        r{proposal.revision}
      </TableCell>
      <TableCell className="px-3 text-xs text-muted-foreground">
        <RelativeTime value={proposal.updatedAt} />
      </TableCell>
      {projectName !== undefined ? (
        <TableCell className="max-w-0 px-3">
          <Link to={routes.project(proposal.projectId)} className="block truncate text-xs hover:underline">
            {projectName}
          </Link>
        </TableCell>
      ) : null}
    </TableRow>
  )
}
