import { Skeleton } from '@/components/ui/skeleton'

/** One scope group with three decisions, matching the list geometry. */
export function DecisionListSkeleton() {
  return (
    <div className="flex flex-col gap-2">
      <Skeleton className="h-4 w-28" />
      <div className="flex flex-col divide-y divide-border/60 rounded-lg ring-1 ring-border">
        {Array.from({ length: 3 }, (_, index) => (
          <div key={index} className="flex flex-col gap-2 px-4 py-3">
            <Skeleton className="h-4 w-64" />
            <Skeleton className="h-3.5 w-full max-w-xl" />
            <div className="flex gap-2">
              <Skeleton className="h-5 w-16" />
              <Skeleton className="h-5 w-20" />
              <Skeleton className="h-3 w-32" />
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
