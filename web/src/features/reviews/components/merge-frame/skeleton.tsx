import { Skeleton } from '@/components/ui/skeleton'

/** Tags, two sentences and the controls: the shape of the frame once GitHub answered. */
export function MergeSkeleton() {
  return (
    <div className="flex flex-col gap-3" role="status" aria-label="Reading the pull request from GitHub">
      <div className="flex flex-wrap gap-1.5">
        <Skeleton className="h-5 w-24 rounded-[4px]" />
        <Skeleton className="h-5 w-20 rounded-[4px]" />
        <Skeleton className="h-5 w-16 rounded-[4px]" />
      </div>
      <div className="flex flex-col gap-1.5">
        <Skeleton className="h-3 w-52 max-w-full" />
        <Skeleton className="h-3 w-40 max-w-full" />
      </div>
      <Skeleton className="h-7 w-full" />
      <Skeleton className="h-8 w-full" />
    </div>
  )
}
