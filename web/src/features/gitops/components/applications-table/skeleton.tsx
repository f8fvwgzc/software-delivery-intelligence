import { Skeleton } from '@/components/ui/skeleton'

/** A few rows in the geometry of the live table. */
export function ApplicationsSkeleton({ rows = 3 }: { rows?: number }) {
  return (
    <div role="status" aria-label="Loading applications">
      <div className="h-9 border-b bg-muted/30" />
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex h-13 items-center gap-4 border-b px-3 last:border-b-0">
          <div className="flex min-w-0 flex-1 flex-col gap-1.5">
            <Skeleton className="h-3 w-36 max-w-full" />
            <Skeleton className="h-2.5 w-52 max-w-full" />
          </div>
          <Skeleton className="h-5 w-16 rounded-[4px]" />
          <Skeleton className="h-5 w-16 rounded-[4px]" />
          <Skeleton className="hidden h-3 w-32 md:block" />
          <Skeleton className="h-3 w-14" />
          <Skeleton className="hidden h-5 w-20 rounded-[4px] lg:block" />
        </div>
      ))}
    </div>
  )
}
