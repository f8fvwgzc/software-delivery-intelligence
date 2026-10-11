import { Skeleton } from '@/components/ui/skeleton'

/** Title line, provenance line and three actions, in the header's geometry. */
export function DiagramHeaderSkeleton() {
  return (
    <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div className="flex flex-col gap-2">
        <Skeleton className="h-6 w-64" />
        <div className="flex items-center gap-2">
          <Skeleton className="h-5 w-24 rounded-md" />
          <Skeleton className="h-5 w-20 rounded-md" />
          <Skeleton className="h-3 w-8" />
          <Skeleton className="h-3 w-14" />
          <Skeleton className="h-3 w-32" />
        </div>
      </div>
      <div className="flex items-center gap-2">
        <Skeleton className="h-7 w-28" />
        <Skeleton className="h-7 w-20" />
        <Skeleton className="size-7" />
      </div>
    </div>
  )
}
