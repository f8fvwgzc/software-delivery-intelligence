import { Skeleton } from '@/components/ui/skeleton'

/** Same rail geometry as the real list: node, line, boxed row. */
export function ExecutionRailSkeleton({ rows = 5 }: { rows?: number }) {
  return (
    <ol className="flex flex-col">
      {Array.from({ length: rows }, (_, index) => (
        <li
          key={index}
          className="relative flex gap-3 pb-5 after:absolute after:top-7 after:bottom-0 after:left-[11px] after:w-px after:bg-border last:pb-0 last:after:hidden"
        >
          <Skeleton className="mt-0.5 size-6 shrink-0 rounded-full" />
          <div className="flex min-w-0 flex-1 flex-col gap-2 rounded-lg px-3 py-2.5 ring-1 ring-border">
            <div className="flex items-center gap-2">
              <Skeleton className="h-4 w-44" />
              <Skeleton className="h-5 w-24 rounded-md" />
              <Skeleton className="ml-auto h-5 w-16 rounded-md" />
            </div>
            <Skeleton className="h-3 w-72 max-w-full" />
          </div>
        </li>
      ))}
    </ol>
  )
}
