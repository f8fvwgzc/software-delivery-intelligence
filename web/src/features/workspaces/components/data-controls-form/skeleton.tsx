import { Skeleton } from '@/components/ui/skeleton'

/** Same frame as the live form: three radio rows, a switch, a toggle, two inputs, a footer. */
export function DataControlsSkeleton() {
  return (
    <div className="flex flex-col gap-5 rounded-lg p-4 ring-1 ring-border" aria-hidden>
      <div className="flex flex-col gap-3">
        <Skeleton className="h-3 w-24" />
        {Array.from({ length: 3 }, (_, index) => (
          <div key={index} className="flex items-center gap-2">
            <Skeleton className="size-4 rounded-full" />
            <Skeleton className="h-3 w-40" />
          </div>
        ))}
        <Skeleton className="h-8 w-48" />
      </div>
      <div className="flex items-center gap-2">
        <Skeleton className="h-[18px] w-8 rounded-full" />
        <Skeleton className="h-3 w-28" />
      </div>
      <Skeleton className="h-7 w-32" />
      <Skeleton className="h-8 w-48" />
      <div className="flex justify-end">
        <Skeleton className="h-7 w-28" />
      </div>
    </div>
  )
}
