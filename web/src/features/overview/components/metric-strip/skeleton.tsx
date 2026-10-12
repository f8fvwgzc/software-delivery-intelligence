import { Skeleton } from '@/components/ui/skeleton'

/** Four stat cells with hairlines between, same grid as the strip. */
export function MetricStripSkeleton() {
  return (
    <div className="grid grid-cols-2 gap-px overflow-hidden rounded-lg bg-border ring-1 ring-border sm:grid-cols-4">
      {Array.from({ length: 4 }, (_, index) => (
        <div key={index} className="flex flex-col gap-2 bg-background px-3 py-2.5">
          <Skeleton className="h-2.5 w-20" />
          <Skeleton className={index === 0 ? 'h-6 w-12' : 'h-5 w-10'} />
          <Skeleton className="h-2.5 w-28 max-w-full" />
        </div>
      ))}
    </div>
  )
}
