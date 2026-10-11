import { Skeleton } from '@/components/ui/skeleton'

/** Same frame and row height as the live table. */
export function MembersTableSkeleton({ rows = 4 }: { rows?: number }) {
  return (
    <div className="flex flex-col overflow-hidden rounded-lg ring-1 ring-border" aria-hidden>
      <div className="h-9 bg-muted/30" />
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex h-11 items-center gap-3 border-t border-border/60 px-3">
          <Skeleton className="size-6 rounded-full" />
          <Skeleton className="h-3 w-28" />
          <Skeleton className="ml-auto h-5 w-16 rounded-md" />
          <Skeleton className="h-7 w-44 rounded-md" />
        </div>
      ))}
    </div>
  )
}
