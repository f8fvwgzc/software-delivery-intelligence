import { Skeleton } from '@/components/ui/skeleton'

/** Eight definition rows, like the configured-facts list. */
export function ConfigPanelSkeleton() {
  return (
    <div className="flex flex-col gap-2">
      {Array.from({ length: 8 }, (_, index) => (
        <div key={index} className="flex items-center justify-between gap-4 border-b border-border/60 pb-2 last:border-0">
          <Skeleton className="h-3 w-20" />
          <Skeleton className="h-4 w-28" />
        </div>
      ))}
      <Skeleton className="mt-1 h-3 w-64 max-w-full" />
    </div>
  )
}
