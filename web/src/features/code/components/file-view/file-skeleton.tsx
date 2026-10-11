import { Skeleton } from '@/components/ui/skeleton'

const WIDTHS = ['42%', '63%', '28%', '71%', '55%', '18%', '66%', '47%', '35%', '58%', '24%', '51%']

/** Header strip and a dozen numbered lines. */
export function FileViewSkeleton() {
  return (
    <div className="flex flex-col">
      <div className="flex min-h-11 items-center gap-3 border-b px-3 py-2">
        <Skeleton className="size-4 rounded-sm" />
        <Skeleton className="h-4 w-56" />
        <Skeleton className="h-4 w-14" />
        <Skeleton className="ml-auto h-6 w-20 rounded-md" />
        <Skeleton className="h-6 w-12 rounded-md" />
      </div>
      <div className="flex flex-col gap-0 py-2">
        {WIDTHS.map((width, index) => (
          <div key={index} className="flex h-5 items-center">
            <span className="w-14 shrink-0 pr-3 text-right font-mono text-[12.5px] tabular-nums text-muted-foreground/40">
              {index + 1}
            </span>
            <Skeleton className="h-3" style={{ width }} />
          </div>
        ))}
      </div>
    </div>
  )
}
