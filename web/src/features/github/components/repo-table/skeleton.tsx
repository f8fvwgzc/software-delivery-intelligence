import { Skeleton } from '@/components/ui/skeleton'

/** Header line plus rows shaped like a repository row: name, chip, time, button. */
export function RepoTableSkeleton({ rows = 8 }: { rows?: number }) {
  return (
    <div className="flex flex-col overflow-hidden rounded-lg ring-1 ring-border">
      <div className="flex h-9 items-center gap-6 border-b bg-muted/30 px-3">
        <Skeleton className="h-3 w-20" />
        <Skeleton className="hidden h-3 w-14 md:block" />
        <Skeleton className="hidden h-3 w-12 sm:block" />
      </div>
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex h-11 items-center gap-6 border-b px-3 last:border-0">
          <div className="flex flex-1 flex-col gap-1.5">
            <Skeleton className="h-3 w-40" />
            <Skeleton className="h-2.5 w-56 max-w-full" />
          </div>
          <Skeleton className="hidden h-4 w-14 md:block" />
          <Skeleton className="hidden h-3 w-16 sm:block" />
          <Skeleton className="h-7 w-20" />
        </div>
      ))}
    </div>
  )
}
