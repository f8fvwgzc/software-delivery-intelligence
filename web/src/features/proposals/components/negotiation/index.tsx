import { QueryBoundary } from '@/components/shared/feedback'
import { Separator } from '@/components/ui/separator'
import type { components } from '@/lib/api/schema'

import { useNegotiate } from '../../api/mutations'
import { Composer } from './composer'
import { ThreadBody } from './thread-body'
import { ThreadSkeleton } from './thread-skeleton'
import { WeightsTable } from './weights-table'

type Proposal = components['schemas']['Proposal']

interface NegotiationPanelProps {
  proposal: Proposal
  readOnly: boolean
}

/**
 * The negotiation thread. Messages change constraints and weights, never
 * the signals, so the agreed constraints and the live weights sit under
 * the composer where the user can see what the ranking is reacting to.
 */
export function NegotiationPanel({ proposal, readOnly }: NegotiationPanelProps) {
  const negotiate = useNegotiate(proposal.id)

  return (
    <aside aria-label="Negotiation" className="flex flex-col rounded-lg bg-card ring-1 ring-border">
      <header className="flex items-center justify-between gap-2 px-4 py-3">
        <h2 className="text-sm font-medium">Negotiation</h2>
        <span className="font-mono text-2xs tabular-nums text-muted-foreground">
          r{proposal.revision} · {proposal.reasoner}
        </span>
      </header>
      <Separator />
      <QueryBoundary fallback={<ThreadSkeleton />}>
        <ThreadBody
          proposalId={proposal.id}
          constraints={proposal.constraints}
          pendingContent={negotiate.isPending ? negotiate.variables : undefined}
        />
      </QueryBoundary>
      <Separator />
      <Composer
        onSend={(content) => negotiate.mutate(content)}
        sending={negotiate.isPending}
        disabled={readOnly}
        disabledHint={readOnly ? 'This proposal is closed. Start a new one to negotiate again.' : undefined}
      />
      {proposal.weights ? (
        <>
          <Separator />
          <WeightsTable weights={proposal.weights} />
        </>
      ) : null}
    </aside>
  )
}
