import { Skeleton } from '@/components/ui/skeleton'

interface FormSkeletonProps {
  /** Number of label + input pairs in the grid. */
  fields: number
  /** Columns at the widest breakpoint, matching the live form. */
  columns?: 2 | 4
  /** A toggle row under the grid (the scanner's cache switch, the reasoner). */
  toggle?: boolean
}

/** Same frame, grid and footer as the live forms, so nothing jumps when data lands. */
export function FormSkeleton({ fields, columns = 2, toggle = false }: FormSkeletonProps) {
  return (
    <div className="flex flex-col gap-4 rounded-lg p-4 ring-1 ring-border">
      <div className={columns === 4 ? 'grid gap-3 sm:grid-cols-2 lg:grid-cols-4' : 'grid gap-4 sm:grid-cols-2'}>
        {Array.from({ length: fields }, (_, index) => (
          <div key={index} className="flex flex-col gap-2">
            <Skeleton className="h-3 w-24" />
            <Skeleton className="h-8 w-full" />
            {columns === 2 ? <Skeleton className="h-3 w-40 max-w-full" /> : null}
          </div>
        ))}
      </div>
      {toggle ? (
        <div className="flex items-center gap-2">
          <Skeleton className="h-[18px] w-8 rounded-full" />
          <Skeleton className="h-4 w-28" />
        </div>
      ) : null}
      <div className="flex justify-end">
        <Skeleton className="h-7 w-24" />
      </div>
    </div>
  )
}

/** Three definition rows and a footnote, like the environment block. */
export function EnvironmentSkeleton() {
  return (
    <div className="flex flex-col gap-3 rounded-lg p-4 ring-1 ring-border">
      {Array.from({ length: 3 }, (_, index) => (
        <div key={index} className="flex items-center justify-between gap-4 border-b border-border/60 pb-2 last:border-0">
          <Skeleton className="h-3 w-24" />
          <Skeleton className="h-5 w-24" />
        </div>
      ))}
      <Skeleton className="h-3 w-56 max-w-full" />
    </div>
  )
}
