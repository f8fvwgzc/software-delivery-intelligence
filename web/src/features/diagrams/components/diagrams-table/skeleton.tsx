import { Skeleton } from '@/components/ui/skeleton'

/** Mirrors the table geometry so the content swap does not shift the layout. */
export function DiagramsTableSkeleton({ rows = 4 }: { rows?: number }) {
  return (
    <div className="overflow-hidden rounded-lg ring-1 ring-border">
      <div className="flex h-10 items-center gap-3 border-b bg-muted/40 px-3">
        <Skeleton className="h-3 w-40" />
        <Skeleton className="h-3 w-16" />
        <Skeleton className="h-3 w-16" />
        <Skeleton className="h-3 w-12" />
        <Skeleton className="h-3 w-14" />
        <Skeleton className="ml-auto h-3 w-14" />
      </div>
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex h-11 items-center gap-3 border-b px-3 last:border-0">
          <Skeleton className="size-4 rounded-sm" />
          <Skeleton className="h-3 w-48" />
          <Skeleton className="h-3 w-20" />
          <Skeleton className="h-5 w-20 rounded-md" />
          <Skeleton className="h-3 w-6" />
          <Skeleton className="h-3 w-14" />
          <Skeleton className="ml-auto h-3 w-16" />
          <Skeleton className="size-6" />
        </div>
      ))}
    </div>
  )
}
