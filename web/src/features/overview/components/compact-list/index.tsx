import { ArrowRightIcon } from 'lucide-react'
import type { ReactNode } from 'react'
import { Link } from 'react-router'

import { Frame } from '@/components/system/frame'
import { Row, RowList, type RowProps } from '@/components/system/row'
import { Skeleton } from '@/components/ui/skeleton'

interface CompactListProps {
  title: ReactNode
  viewAll: { to: string; label: string }
  /** Rendered instead of the list when there is nothing to show. */
  empty?: ReactNode
  children?: ReactNode
}

/** A framed list of 5-ish rows with a "view all" link: a panel, not a card grid. */
export function CompactList({ title, viewAll, empty, children }: CompactListProps) {
  return (
    <Frame
      title={title}
      padding="none"
      actions={
        <Link
          to={viewAll.to}
          className="inline-flex h-6 items-center gap-1 rounded-md px-1.5 text-xs text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          {viewAll.label}
          <ArrowRightIcon className="size-3" aria-hidden />
        </Link>
      }
    >
      {empty ?? <RowList stagger>{children}</RowList>}
    </Frame>
  )
}

type CompactListRowProps = Pick<RowProps, 'to' | 'leading' | 'title' | 'meta' | 'trailing' | 'className'>

export function CompactListRow(props: CompactListRowProps) {
  return (
    <li>
      <Row size="md" {...props} />
    </li>
  )
}

/** Same frame chrome and five row-shaped lines. */
export function CompactListSkeleton({ title, rows = 5 }: { title: ReactNode; rows?: number }) {
  return (
    <Frame title={title} padding="none" actions={<Skeleton className="h-5 w-16" />}>
      <div className="flex flex-col divide-y divide-border">
        {Array.from({ length: rows }, (_, index) => (
          <div key={index} className="flex h-11 items-center gap-3 px-3">
            <Skeleton className="size-1.5 rounded-full" />
            <div className="flex flex-1 flex-col gap-1.5">
              <Skeleton className="h-3.5 w-48 max-w-full" />
              <Skeleton className="h-2.5 w-32" />
            </div>
            <Skeleton className="h-3 w-14" />
          </div>
        ))}
      </div>
    </Frame>
  )
}
