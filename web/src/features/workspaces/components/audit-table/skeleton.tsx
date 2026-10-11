import { Skeleton } from '@/components/ui/skeleton'

export function AuditTableSkeleton({ rows = 8 }: { rows?: number }) {
  return (
    <div className="flex flex-col overflow-hidden rounded-lg ring-1 ring-border" aria-hidden>
      <div className="h-9 bg-muted/30" />
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex h-9 items-center gap-4 border-t border-border/60 px-3">
          <Skeleton className="h-3 w-24" />
          <Skeleton className="h-3 w-16" />
          <Skeleton className="h-4 w-24 rounded-md" />
          <Skeleton className="h-3 w-32" />
          <Skeleton className="h-3 flex-1" />
        </div>
      ))}
    </div>
  )
}
