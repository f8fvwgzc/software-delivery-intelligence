import { Skeleton } from '@/components/ui/skeleton'

/** Header line, three service lines, footer line. */
export function StatusStripSkeleton() {
  return (
    <div className="flex h-full min-h-24 flex-col justify-between gap-2 rounded-lg px-3 py-2.5 ring-1 ring-border">
      <div className="flex items-center justify-between">
        <Skeleton className="h-3 w-14" />
        <Skeleton className="h-3 w-20" />
      </div>
      <div className="flex flex-col gap-1.5">
        {Array.from({ length: 3 }, (_, index) => (
          <div key={index} className="flex items-center justify-between">
            <Skeleton className="h-3 w-20" />
            <Skeleton className="h-3 w-10" />
          </div>
        ))}
      </div>
      <Skeleton className="h-2.5 w-28" />
    </div>
  )
}
