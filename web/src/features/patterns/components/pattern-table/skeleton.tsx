import { Skeleton } from '@/components/ui/skeleton'

export function PatternTableSkeleton({ rows = 8 }: { rows?: number }) {
  return (
    <div className="flex flex-col rounded-lg ring-1 ring-border">
      <div className="flex h-10 items-center gap-6 border-b px-3">
        <Skeleton className="h-3 w-24" />
        <Skeleton className="h-3 w-16" />
        <Skeleton className="h-3 w-28" />
        <Skeleton className="h-3 w-20" />
      </div>
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex h-11 items-center gap-6 border-b px-3 last:border-0">
          <Skeleton className="size-4 rounded-md" />
          <Skeleton className="h-3 w-48" />
          <Skeleton className="h-3 w-16" />
          <div className="flex gap-1">
            <Skeleton className="h-5 w-12" />
            <Skeleton className="h-5 w-14" />
          </div>
          <Skeleton className="h-1.5 w-32" />
          <Skeleton className="h-3 w-12" />
        </div>
      ))}
    </div>
  )
}
