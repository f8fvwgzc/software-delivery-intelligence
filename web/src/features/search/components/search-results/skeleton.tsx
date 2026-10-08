import { Skeleton } from '@/components/ui/skeleton'

export function SearchResultsSkeleton({ rows = 5 }: { rows?: number }) {
  return (
    <div className="flex flex-col gap-2">
      <Skeleton className="h-3 w-28" />
      <div className="flex flex-col divide-y divide-border/60 rounded-lg ring-1 ring-border">
        {Array.from({ length: rows }, (_, index) => (
          <div key={index} className="flex gap-3 px-4 py-3">
            <Skeleton className="mt-0.5 size-4 rounded-sm" />
            <div className="flex flex-1 flex-col gap-1.5">
              <Skeleton className="h-3.5 w-1/2" />
              <Skeleton className="h-3 w-1/3" />
              <Skeleton className="h-3 w-full" />
              <Skeleton className="h-3 w-5/6" />
            </div>
            <Skeleton className="hidden h-1.5 w-28 sm:block" />
          </div>
        ))}
      </div>
    </div>
  )
}
