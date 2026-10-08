import { Skeleton } from '@/components/ui/skeleton'

/** Panel header plus dense rows, same height as the live log. */
export function EventLogSkeleton() {
  return (
    <div className="flex flex-col overflow-hidden rounded-lg ring-1 ring-border">
      <div className="flex h-10 items-center gap-2 border-b px-3">
        <Skeleton className="h-3 w-16" />
        <Skeleton className="ml-auto h-7 w-28 rounded-md" />
        <Skeleton className="h-7 w-16 rounded-md" />
      </div>
      <div className="flex h-[32rem] flex-col gap-1.5 px-3 py-2">
        {Array.from({ length: 14 }, (_, index) => (
          <div key={index} className="flex items-center gap-2">
            <Skeleton className="h-3 w-6" />
            <Skeleton className="h-3 w-14" />
            <Skeleton className="h-3 w-12" />
            <Skeleton className="h-3 flex-1" style={{ maxWidth: `${55 + ((index * 17) % 40)}%` }} />
          </div>
        ))}
      </div>
    </div>
  )
}
