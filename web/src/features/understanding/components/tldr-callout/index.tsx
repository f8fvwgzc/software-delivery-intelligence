import { RelativeTime } from '@/components/shared/data/relative-time'
import { TechChips } from '@/components/shared/data/tech-chip'
import { Skeleton } from '@/components/ui/skeleton'
import { testIds } from '@/test-ids'

import type { Understanding } from '../../api/queries'
import { shortSha } from '../../lib/github'

/**
 * The one-line answer to "what is this system?". A faint brand tint marks
 * it as the hero of the page; everything below is detail.
 */
export function TldrCallout({ understanding }: { understanding: Understanding }) {
  return (
    <div data-testid={testIds.understanding.tldr} className="flex flex-col gap-3 rounded-lg bg-brand/5 p-4 ring-1 ring-brand/10">
      <p className="text-base leading-snug font-medium text-balance">
        {understanding.tldr || 'No summary was generated for this commit.'}
      </p>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        {understanding.stack.length > 0 ? <TechChips items={understanding.stack} max={8} /> : null}
        <span className="font-mono text-xs text-muted-foreground">
          {understanding.commitSha ? `${shortSha(understanding.commitSha)} · ` : null}
          generated <RelativeTime value={understanding.generatedAt} />
        </span>
      </div>
    </div>
  )
}

export function TldrCalloutSkeleton() {
  return (
    <div className="flex flex-col gap-3 rounded-lg bg-muted/40 p-4 ring-1 ring-border">
      <Skeleton className="h-5 w-3/4" />
      <div className="flex items-center gap-2">
        <Skeleton className="h-5 w-16" />
        <Skeleton className="h-5 w-14" />
        <Skeleton className="h-5 w-20" />
        <Skeleton className="ml-2 h-4 w-40" />
      </div>
    </div>
  )
}
