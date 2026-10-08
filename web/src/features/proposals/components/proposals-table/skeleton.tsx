import { Skeleton } from '@/components/ui/skeleton'

/** Mirrors the table geometry: header row plus a few 44px rows. */
export function ProposalsTableSkeleton({ rows = 6 }: { rows?: number }) {
  return (
    <div className="flex flex-col rounded-lg ring-1 ring-border">
      <div className="flex h-10 items-center gap-6 border-b px-3">
        <Skeleton className="h-3 w-12" />
        <Skeleton className="h-3 w-40" />
        <Skeleton className="h-3 w-16" />
        <Skeleton className="h-3 w-32" />
      </div>
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex h-11 items-center gap-6 border-b px-3 last:border-0">
          <Skeleton className="h-3 w-16" />
          <Skeleton className="h-3 w-56" />
          <Skeleton className="h-3 w-20" />
          <Skeleton className="h-1.5 w-48" />
          <Skeleton className="h-3 w-10" />
        </div>
      ))}
    </div>
  )
}
