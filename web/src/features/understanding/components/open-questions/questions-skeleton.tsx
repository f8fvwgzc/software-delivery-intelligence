import { Skeleton } from '@/components/ui/skeleton'

/** Two question frames side by side, like the open-questions grid. */
export function OpenQuestionsSkeleton() {
  return (
    <div className="grid gap-4 md:grid-cols-2">
      {Array.from({ length: 2 }, (_, index) => (
        <div key={index} className="flex flex-col gap-4 rounded-lg p-4 ring-1 ring-border">
          <div className="flex flex-col gap-1.5">
            <Skeleton className="h-4 w-3/4" />
            <Skeleton className="h-3 w-full" />
          </div>
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-2">
              <Skeleton className="size-4 rounded-full" />
              <Skeleton className="h-4 w-28" />
            </div>
            <div className="flex items-center gap-2">
              <Skeleton className="size-4 rounded-full" />
              <Skeleton className="h-4 w-20" />
            </div>
          </div>
          <div className="flex justify-end">
            <Skeleton className="h-7 w-16" />
          </div>
        </div>
      ))}
    </div>
  )
}
