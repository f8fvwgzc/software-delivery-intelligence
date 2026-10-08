import { Skeleton } from '@/components/ui/skeleton'

interface KeyValueSkeletonProps {
  rows?: number
}

/** Mirrors the geometry of <KeyValue>: one label/value line per row. */
export function KeyValueSkeleton({ rows = 5 }: KeyValueSkeletonProps) {
  return (
    <div className="flex flex-col gap-2">
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex items-center justify-between border-b border-border/60 pb-2 last:border-0">
          <Skeleton className="h-3 w-24" />
          <Skeleton className="h-3.5 w-16" />
        </div>
      ))}
    </div>
  )
}
