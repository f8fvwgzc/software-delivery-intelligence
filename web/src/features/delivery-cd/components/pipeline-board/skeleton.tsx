import { Skeleton } from '@/components/ui/skeleton'

/** Three stage columns with their blocks, the shape the board takes once loaded. */
export function PipelineBoardSkeleton({ stages = 3 }: { stages?: number }) {
  return (
    <div className="flex flex-col gap-3 p-px lg:flex-row" role="status" aria-label="Loading the pipeline">
      {Array.from({ length: stages }, (_, index) => (
        <div key={index} className="flex min-w-0 flex-1 flex-col rounded-md ring-1 ring-border">
          <div className="flex h-8 items-center gap-2 border-b px-3">
            <Skeleton className="h-3 w-20" />
            <Skeleton className="size-1.5 rounded-full" />
          </div>
          <div className="flex flex-col gap-2 border-b px-3 py-2.5">
            <Skeleton className="h-2.5 w-12" />
            <Skeleton className="h-4 w-24" />
            <Skeleton className="h-3 w-36 max-w-full" />
          </div>
          <div className="flex items-center gap-2 px-3 py-2.5">
            <div className="flex flex-1 flex-col gap-2">
              <Skeleton className="h-2.5 w-8" />
              <Skeleton className="h-4 w-20" />
            </div>
            <Skeleton className="h-7 w-20 rounded-md" />
          </div>
          <div className="flex h-8 items-center gap-2 border-t px-3">
            <Skeleton className="h-4 w-20 rounded-[4px]" />
            <Skeleton className="ml-auto h-3 w-12" />
          </div>
        </div>
      ))}
    </div>
  )
}
