import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'

import { CANVAS_HEIGHT_CLASS } from '../canvas-skeleton'

/** Toolbar line, then the two panes of the editor, while the Mermaid chunk loads. */
export function MermaidSkeleton() {
  return (
    <div className="flex flex-col gap-3" aria-hidden>
      <div className="flex items-center justify-between">
        <Skeleton className="h-3 w-40" />
        <div className="flex gap-2">
          <Skeleton className="h-7 w-16" />
          <Skeleton className="h-7 w-16" />
        </div>
      </div>
      <div className={cn('grid gap-3 lg:grid-cols-2', CANVAS_HEIGHT_CLASS)}>
        <div className="flex flex-col gap-1.5">
          <Skeleton className="h-3 w-12" />
          <div className="flex flex-1 flex-col gap-2 rounded-lg border border-input p-3">
            {[40, 56, 72, 64, 48, 80, 60, 36].map((width, index) => (
              <Skeleton key={index} className="h-2.5" style={{ width: `${width}%` }} />
            ))}
          </div>
        </div>
        <div className="flex flex-col gap-1.5">
          <Skeleton className="h-3 w-14" />
          <Skeleton className="flex-1 rounded-lg" />
        </div>
      </div>
    </div>
  )
}
