import { Frame } from '@/components/system/frame'
import { Skeleton } from '@/components/ui/skeleton'

/** Section title, filter chips, header line and four rows shaped like queue rows. */
export function WorkQueueSkeleton({ rows = 4 }: { rows?: number }) {
  return (
    <Frame
      title="Work queue"
      description="Oldest first. Paused executions, approvals and open questions."
      actions={
        <div className="flex gap-1">
          {Array.from({ length: 4 }, (_, index) => (
            <Skeleton key={index} className="h-6 w-16" />
          ))}
        </div>
      }
      padding="none"
    >
      <div className="flex flex-col">
        <div className="flex h-9 items-center gap-6 border-b bg-muted/30 px-3">
          <Skeleton className="h-3 w-10" />
          <Skeleton className="h-3 w-14" />
          <Skeleton className="h-3 w-16" />
          <Skeleton className="ml-auto hidden h-3 w-14 sm:block" />
        </div>
        {Array.from({ length: rows }, (_, index) => (
          <div key={index} className="flex h-12 items-center gap-6 border-b px-3 last:border-0">
            <Skeleton className="h-3 w-16" />
            <Skeleton className="h-3.5 w-24" />
            <div className="flex flex-1 flex-col gap-1.5">
              <Skeleton className="h-3.5 w-48 max-w-full" />
              <Skeleton className="h-2.5 w-64 max-w-full" />
            </div>
            <Skeleton className="hidden h-3 w-16 sm:block" />
            <Skeleton className="h-7 w-20" />
          </div>
        ))}
      </div>
    </Frame>
  )
}
