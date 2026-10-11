import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'

interface PageSkeletonProps {
  /** Number of content blocks below the header. */
  blocks?: number
  className?: string
}

/** Generic loading shape for a page: header line, then a few cards. */
export function PageSkeleton({ blocks = 3, className }: PageSkeletonProps) {
  return (
    <div className={cn('flex w-full min-w-0 flex-col gap-6 px-4 py-5 md:px-6 md:py-6', className)}>
      <div className="flex flex-col gap-2">
        <Skeleton className="h-6 w-48" />
        <Skeleton className="h-4 w-80 max-w-full" />
      </div>
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
        {Array.from({ length: blocks }, (_, index) => (
          <Skeleton key={index} className="h-32 rounded-lg" />
        ))}
      </div>
    </div>
  )
}

/** Loading shape for a list of rows. */
export function ListSkeleton({ rows = 5, className }: { rows?: number; className?: string }) {
  return (
    <div className={cn('flex flex-col gap-2', className)}>
      {Array.from({ length: rows }, (_, index) => (
        <Skeleton key={index} className="h-12 rounded-lg" />
      ))}
    </div>
  )
}
