import { ArrowRightIcon } from 'lucide-react'
import type { ReactNode } from 'react'
import { Link } from 'react-router'

import { Section } from '@/components/shared/page'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'

interface CompactListProps {
  title: ReactNode
  viewAll: { to: string; label: string }
  /** Rendered instead of the list when there is nothing to show. */
  empty?: ReactNode
  children?: ReactNode
}

/** A titled list of 5-ish rows with a "view all" link: a panel, not a card grid. */
export function CompactList({ title, viewAll, empty, children }: CompactListProps) {
  return (
    <Section
      title={title}
      actions={
        <Button asChild variant="ghost" size="xs" className="text-muted-foreground">
          <Link to={viewAll.to}>
            {viewAll.label}
            <ArrowRightIcon data-icon="inline-end" />
          </Link>
        </Button>
      }
    >
      {empty ?? <ul className="divide-y divide-border/60 overflow-hidden rounded-lg ring-1 ring-border">{children}</ul>}
    </Section>
  )
}

interface CompactListRowProps {
  to: string
  leading?: ReactNode
  title: ReactNode
  meta?: ReactNode
  trailing?: ReactNode
  className?: string
}

export function CompactListRow({ to, leading, title, meta, trailing, className }: CompactListRowProps) {
  return (
    <li>
      <Link
        to={to}
        className={cn(
          'flex min-h-10 items-center gap-3 px-3 py-1.5 text-sm transition-colors hover:bg-muted/50 focus-visible:bg-muted/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset',
          className,
        )}
      >
        {leading}
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate">{title}</span>
          {meta ? <span className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">{meta}</span> : null}
        </span>
        {trailing ? <span className="shrink-0 font-mono text-xs text-muted-foreground">{trailing}</span> : null}
      </Link>
    </li>
  )
}

/** Same section chrome and five row-shaped lines. */
export function CompactListSkeleton({ title, rows = 5 }: { title: ReactNode; rows?: number }) {
  return (
    <Section title={title} actions={<Skeleton className="h-6 w-16" />}>
      <div className="flex flex-col divide-y divide-border/60 overflow-hidden rounded-lg ring-1 ring-border">
        {Array.from({ length: rows }, (_, index) => (
          <div key={index} className="flex h-10 items-center gap-3 px-3">
            <Skeleton className="size-1.5 rounded-full" />
            <div className="flex flex-1 flex-col gap-1.5">
              <Skeleton className="h-3.5 w-48 max-w-full" />
              <Skeleton className="h-2.5 w-32" />
            </div>
            <Skeleton className="h-3 w-14" />
          </div>
        ))}
      </div>
    </Section>
  )
}
