import { Skeleton } from '@/components/ui/skeleton'

export function DocumentListSkeleton({ rows = 4 }: { rows?: number }) {
  return (
    <div className="flex flex-col divide-y divide-border/60 rounded-lg ring-1 ring-border">
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex flex-col gap-1.5 px-3 py-2.5">
          <Skeleton className="h-3.5 w-3/4" />
          <Skeleton className="h-3 w-1/2" />
        </div>
      ))}
    </div>
  )
}
