import { Skeleton } from '@/components/ui/skeleton'

/** Hero block plus three support blocks, same grid as the strip. */
export function MetricStripSkeleton() {
  return (
    <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-[minmax(0,1.6fr)_repeat(3,minmax(0,1fr))]">
      <div className="flex flex-col gap-2 rounded-lg px-4 py-3 ring-1 ring-border">
        <Skeleton className="h-3 w-20" />
        <Skeleton className="h-7 w-12" />
        <Skeleton className="h-3 w-40 max-w-full" />
      </div>
      {Array.from({ length: 3 }, (_, index) => (
        <div key={index} className="flex flex-col gap-2 rounded-lg px-3 py-3 ring-1 ring-border">
          <Skeleton className="h-3 w-24" />
          <Skeleton className="h-6 w-10" />
          <Skeleton className="h-2.5 w-20" />
        </div>
      ))}
    </div>
  )
}
