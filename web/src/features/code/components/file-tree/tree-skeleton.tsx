import { Skeleton } from '@/components/ui/skeleton'

import { INDENT, ROW_CLASS } from './constants'

const ROWS: Array<{ depth: number; width: string }> = [
  { depth: 0, width: '3.5rem' },
  { depth: 0, width: '5rem' },
  { depth: 1, width: '4rem' },
  { depth: 1, width: '6rem' },
  { depth: 2, width: '5.5rem' },
  { depth: 2, width: '4.5rem' },
  { depth: 1, width: '3rem' },
  { depth: 0, width: '4.5rem' },
  { depth: 0, width: '6.5rem' },
  { depth: 0, width: '5rem' },
  { depth: 0, width: '3.5rem' },
  { depth: 0, width: '4rem' },
]

/** Filter box and a dozen indented rows, in the live geometry. */
export function FileTreeSkeleton() {
  return (
    <div className="flex flex-col gap-2">
      <Skeleton className="h-7 rounded-lg" />
      <div className="flex flex-col">
        {ROWS.map((row, index) => (
          <div key={index} className={`${ROW_CLASS} flex items-center gap-1.5`} style={{ paddingLeft: row.depth * INDENT + 24 }}>
            <Skeleton className="size-4 rounded-sm" />
            <Skeleton className="h-3" style={{ width: row.width }} />
          </div>
        ))}
      </div>
    </div>
  )
}
