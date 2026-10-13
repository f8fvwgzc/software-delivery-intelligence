import { Skeleton } from '@/components/ui/skeleton'

/** The filter row and a few table rows, in the geometry of the live table. */
export function VulnerabilitiesSkeleton({ rows = 5 }: { rows?: number }) {
  return (
    <div role="status" aria-label="Loading vulnerabilities">
      <div className="flex min-h-9 items-center gap-1.5 border-b px-3 py-1.5">
        <Skeleton className="h-5 w-12 rounded-[4px]" />
        <Skeleton className="h-5 w-16 rounded-[4px]" />
        <Skeleton className="h-5 w-14 rounded-[4px]" />
      </div>
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex h-13 items-center gap-4 border-b px-3 last:border-b-0">
          <Skeleton className="h-5 w-14 rounded-[4px]" />
          <div className="flex min-w-0 flex-1 flex-col gap-1.5">
            <Skeleton className="h-3 w-40 max-w-full" />
            <Skeleton className="h-2.5 w-64 max-w-full" />
          </div>
          <Skeleton className="hidden h-3 w-28 sm:block" />
          <Skeleton className="hidden h-3 w-32 md:block" />
          <Skeleton className="h-3 w-12" />
        </div>
      ))}
    </div>
  )
}
