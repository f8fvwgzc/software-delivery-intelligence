import type { RunFailure } from '../../lib/types'

interface FailureDetailsProps {
  failure: RunFailure
}

/** Root cause of a failed run: where, what, and what to try. */
export function FailureDetails({ failure }: FailureDetailsProps) {
  return (
    <dl className="sticky left-0 mx-3 my-2 grid max-w-[calc(100cqw-1.5rem)] gap-x-6 gap-y-2 rounded-md bg-destructive/[0.03] px-4 py-3 text-xs ring-1 ring-destructive/15 sm:grid-cols-[8rem_minmax(0,1fr)]">
      <dt className="text-muted-foreground">Failed at</dt>
      <dd className="font-mono">
        {failure.job}
        <span className="text-muted-foreground"> › </span>
        {failure.step}
      </dd>
      <dt className="text-muted-foreground">What went wrong</dt>
      <dd className="whitespace-pre-wrap">{failure.summary || 'No summary recorded.'}</dd>
      <dt className="text-muted-foreground">Suggested fix</dt>
      <dd className="whitespace-pre-wrap">{failure.suggestedFix || 'No suggestion recorded.'}</dd>
    </dl>
  )
}
