import { Skeleton } from '@/components/ui/skeleton'

export function PoliciesTableSkeleton() {
  return (
    <div className="flex flex-col gap-6">
      {[4, 6, 8].map((rows, group) => (
        <div key={group} className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <Skeleton className="size-1.5 rounded-full" />
            <Skeleton className="h-4 w-16" />
            <Skeleton className="h-3 w-56" />
          </div>
          <div className="overflow-hidden rounded-lg ring-1 ring-border">
            <div className="flex h-9 items-center gap-3 border-b bg-muted/40 px-3">
              <Skeleton className="h-3 w-16" />
              <Skeleton className="h-3 w-10" />
              <Skeleton className="h-3 w-16" />
            </div>
            {Array.from({ length: rows }, (_, index) => (
              <div key={index} className="flex h-12 items-center gap-3 border-b px-3 last:border-0">
                <div className="flex w-64 flex-col gap-1">
                  <Skeleton className="h-3.5 w-40" />
                  <Skeleton className="h-3 w-56" />
                </div>
                <Skeleton className="h-3 w-28" />
                <Skeleton className="h-7 w-32 rounded-md" />
                <Skeleton className="ml-auto h-3 w-12" />
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  )
}
