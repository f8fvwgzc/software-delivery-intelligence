import { Skeleton } from '@/components/ui/skeleton'

/** Toolbar line, then a heading and paragraphs of prose. */
export function DocumentViewerSkeleton() {
  return (
    <div className="flex flex-col rounded-lg ring-1 ring-border">
      <div className="flex items-start justify-between gap-4 px-4 py-3 md:px-6">
        <div className="flex flex-col gap-1.5">
          <Skeleton className="h-3 w-24" />
          <Skeleton className="h-5 w-64" />
          <Skeleton className="h-3 w-40" />
        </div>
        <div className="flex gap-2">
          <Skeleton className="h-7 w-16" />
          <Skeleton className="h-7 w-28" />
        </div>
      </div>
      <div className="flex flex-col gap-3 border-t px-4 py-5 md:px-6">
        <Skeleton className="h-5 w-1/2" />
        <Skeleton className="h-3.5 w-full" />
        <Skeleton className="h-3.5 w-11/12" />
        <Skeleton className="h-3.5 w-4/5" />
        <Skeleton className="mt-3 h-4 w-1/3" />
        <Skeleton className="h-3.5 w-full" />
        <Skeleton className="h-3.5 w-3/4" />
      </div>
    </div>
  )
}
