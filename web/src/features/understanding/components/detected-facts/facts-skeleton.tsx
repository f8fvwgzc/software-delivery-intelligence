import { Skeleton } from '@/components/ui/skeleton'

/** Two groups of definition rows, matching the live list's grid. */
export function DetectedFactsSkeleton() {
  return (
    <div className="flex flex-col gap-5">
      {Array.from({ length: 2 }, (_, group) => (
        <div key={group} className="flex flex-col gap-2">
          <Skeleton className="h-3 w-24" />
          <div className="grid grid-cols-[minmax(7rem,11rem)_minmax(0,1fr)] gap-x-3 gap-y-3">
            {Array.from({ length: 3 }, (_, row) => (
              <div key={row} className="contents">
                <Skeleton className="h-4 w-24" />
                <div className="flex flex-col gap-1">
                  <Skeleton className="h-4 w-32" />
                  <Skeleton className="h-3 w-56 max-w-full" />
                </div>
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  )
}

/** Rows with a thin bar, matching the inferred list. */
export function InferredFactsSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      {Array.from({ length: 4 }, (_, index) => (
        <div key={index} className="flex flex-col gap-1.5">
          <div className="flex items-center justify-between">
            <Skeleton className="h-4 w-36" />
            <Skeleton className="h-3 w-14" />
          </div>
          <Skeleton className="h-1.5 w-full rounded-full" />
          <Skeleton className="h-3 w-48 max-w-full" />
        </div>
      ))}
    </div>
  )
}
