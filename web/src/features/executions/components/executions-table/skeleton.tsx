import { Skeleton } from '@/components/ui/skeleton'

interface ExecutionsTableSkeletonProps {
  rows?: number
  showProject?: boolean
}

/** Mirrors the table geometry so the content swap does not shift the layout. */
export function ExecutionsTableSkeleton({ rows = 6, showProject = true }: ExecutionsTableSkeletonProps) {
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <Skeleton className="h-7 w-64 rounded-lg" />
        <Skeleton className="h-3 w-12" />
      </div>
      <div className="overflow-hidden rounded-lg ring-1 ring-border">
      <div className="flex h-10 items-center gap-3 border-b bg-muted/40 px-3">
        <Skeleton className="h-3 w-8" />
        <Skeleton className="h-3 w-40" />
        {showProject && <Skeleton className="h-3 w-24" />}
        <Skeleton className="h-3 w-16" />
        <Skeleton className="ml-auto h-3 w-14" />
      </div>
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex h-11 items-center gap-3 border-b px-3 last:border-0">
          <Skeleton className="h-3 w-8" />
          <Skeleton className="h-3 w-48" />
          {showProject && <Skeleton className="h-3 w-24" />}
          <Skeleton className="h-5 w-20 rounded-md" />
          <div className="flex w-44 flex-col gap-1.5">
            <Skeleton className="h-3 w-14" />
            <Skeleton className="h-1 w-full rounded-full" />
          </div>
          <Skeleton className="ml-auto h-3 w-16" />
        </div>
      ))}
      </div>
    </div>
  )
}
