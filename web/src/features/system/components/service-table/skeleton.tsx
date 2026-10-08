import { Skeleton } from '@/components/ui/skeleton'

/** Three rows (one per backing service), same column widths as the table. */
export function ServiceTableSkeleton() {
  return (
    <div className="flex flex-col overflow-hidden rounded-lg ring-1 ring-border">
      <div className="flex h-9 items-center gap-6 border-b bg-muted/30 px-3">
        <Skeleton className="h-3 w-14" />
        <Skeleton className="h-3 w-12" />
        <Skeleton className="ml-auto h-3 w-12" />
        <Skeleton className="h-3 w-32" />
      </div>
      {Array.from({ length: 3 }, (_, index) => (
        <div key={index} className="flex h-10 items-center gap-6 border-b px-3 last:border-0">
          <Skeleton className="h-3.5 w-20" />
          <Skeleton className="h-3.5 w-16" />
          <Skeleton className="ml-auto h-3 w-10" />
          <Skeleton className="h-3 w-40" />
        </div>
      ))}
    </div>
  )
}
