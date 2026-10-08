import { Skeleton } from '@/components/ui/skeleton'

export function ApprovalQueueSkeleton({ rows = 3 }: { rows?: number }) {
  return (
    <div className="flex flex-col divide-y rounded-lg ring-1 ring-border">
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex flex-col gap-3 px-4 py-3 md:flex-row md:items-start md:gap-6">
          <div className="flex flex-1 flex-col gap-2">
            <Skeleton className="h-3 w-56" />
            <div className="flex items-center gap-2">
              <Skeleton className="h-4 w-40" />
              <Skeleton className="h-5 w-24 rounded-md" />
            </div>
            <Skeleton className="h-3 w-80 max-w-full" />
            <Skeleton className="h-3 w-48" />
          </div>
          <div className="flex gap-2 md:pt-5">
            <Skeleton className="h-7 w-18 rounded-md" />
            <Skeleton className="h-7 w-20 rounded-md" />
          </div>
        </div>
      ))}
    </div>
  )
}
