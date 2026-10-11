import { Skeleton } from '@/components/ui/skeleton'

/** Hero number, stage badge, then eight bar rows with the live list's spacing. */
export function DeliveryMaturitySkeleton() {
  return (
    <div className="flex flex-col gap-5">
      <div className="flex items-end gap-4">
        <Skeleton className="h-8 w-16" />
        <Skeleton className="mb-0.5 h-5 w-20" />
        <Skeleton className="mb-0.5 h-4 w-32" />
      </div>
      <div className="flex flex-col">
        {Array.from({ length: 8 }, (_, index) => (
          <div key={index} className="flex flex-col gap-1.5 border-b border-border/60 py-3 first:pt-0 last:border-0 last:pb-0">
            <div className="flex items-center gap-2">
              <Skeleton className="h-3 w-28" />
              <Skeleton className="h-1.5 flex-1 rounded-full" />
              <Skeleton className="h-3 w-8" />
            </div>
            <Skeleton className="ml-30 h-3 w-48 max-w-full" />
          </div>
        ))}
      </div>
    </div>
  )
}
