import type { ComponentType, ReactNode } from 'react'

import { EmptyState } from '@/components/shared/feedback/empty-state'
import { QueryBoundary } from '@/components/shared/feedback/query-boundary'
import { Skeleton } from '@/components/ui/skeleton'

import { Frame, type FrameProps } from './frame'

export interface PanelProps extends Omit<FrameProps, 'children'> {
  /** Shown inside the frame while the children suspend. Defaults to rows of lines. */
  skeleton?: ReactNode
  /** Lines in the default skeleton. */
  skeletonRows?: number
  children: ReactNode
}

/**
 * A Frame whose body is its own Suspense and error boundary: the header
 * paints at once, the body shows a skeleton until its `useSuspenseQuery`
 * resolves, and a failure renders a small retry inside the frame instead
 * of taking the page down. Children render `<PanelEmpty>` when the data is
 * there but empty.
 */
export function Panel({ skeleton, skeletonRows = 4, children, ...frame }: PanelProps) {
  return (
    <Frame {...frame}>
      <QueryBoundary compact fallback={skeleton ?? <PanelSkeleton rows={skeletonRows} />}>
        {children}
      </QueryBoundary>
    </Frame>
  )
}

/** Rows of two lines, the generic loading shape of a panel body. */
export function PanelSkeleton({ rows = 4, className }: { rows?: number; className?: string }) {
  return (
    <div className={className} role="status" aria-label="Loading">
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex h-10 items-center gap-3 px-3">
          <Skeleton className="size-1.5 rounded-full" />
          <div className="flex flex-1 flex-col gap-1.5">
            <Skeleton className="h-3 w-48 max-w-full" />
            <Skeleton className="h-2.5 w-28" />
          </div>
          <Skeleton className="h-3 w-12" />
        </div>
      ))}
    </div>
  )
}

interface PanelEmptyProps {
  icon?: ComponentType<{ className?: string }>
  title: ReactNode
  description?: ReactNode
  action?: ReactNode
  className?: string
}

/** The empty state of a panel body: small, centred, no dashed box. */
export function PanelEmpty(props: PanelEmptyProps) {
  return <EmptyState compact {...props} />
}
