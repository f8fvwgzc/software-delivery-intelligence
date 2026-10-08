import { Skeleton } from '@/components/ui/skeleton'

/** Two turns of a thread: a short user line on the right, a reply on the left. */
export function ThreadSkeleton() {
  return (
    <div className="flex min-h-40 flex-col gap-3 px-4 py-3">
      <div className="flex justify-end">
        <Skeleton className="h-9 w-3/5 rounded-lg" />
      </div>
      <div className="flex flex-col gap-1.5">
        <Skeleton className="h-3 w-full" />
        <Skeleton className="h-3 w-11/12" />
        <Skeleton className="h-3 w-2/3" />
        <div className="flex gap-1 pt-1">
          <Skeleton className="h-5 w-20" />
          <Skeleton className="h-5 w-24" />
        </div>
      </div>
    </div>
  )
}
