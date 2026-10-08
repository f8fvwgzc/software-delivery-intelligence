import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'

/** Height shared by the real canvas and its skeleton so nothing jumps when data arrives. */
export const CANVAS_HEIGHT_CLASS = 'h-[max(420px,calc(100dvh-15rem))]'

/** Three ranks of node boxes joined by faint links, like a laid-out graph. */
const RANKS: readonly (readonly number[])[] = [
  [18, 50, 82],
  [10, 34, 58, 82],
  [30, 66],
]
const RANK_LEFT = [6, 40, 74] as const

interface LinkSpec {
  from: readonly [rank: number, index: number]
  to: readonly [rank: number, index: number]
}

const LINKS: readonly LinkSpec[] = [
  { from: [0, 0], to: [1, 0] },
  { from: [0, 0], to: [1, 1] },
  { from: [0, 1], to: [1, 1] },
  { from: [0, 1], to: [1, 2] },
  { from: [0, 2], to: [1, 3] },
  { from: [1, 1], to: [2, 0] },
  { from: [1, 2], to: [2, 0] },
  { from: [1, 3], to: [2, 1] },
]

const BOX_WIDTH = 20 // percent
const BOX_HEIGHT = 7 // percent

function linkPath({ from, to }: LinkSpec) {
  const x1 = RANK_LEFT[from[0]] + BOX_WIDTH
  const y1 = RANKS[from[0]][from[1]] + BOX_HEIGHT / 2
  const x2 = RANK_LEFT[to[0]]
  const y2 = RANKS[to[0]][to[1]] + BOX_HEIGHT / 2
  const mid = (x1 + x2) / 2
  return `M ${x1} ${y1} C ${mid} ${y1}, ${mid} ${y2}, ${x2} ${y2}`
}

/** The canvas alone, used while the React Flow chunk loads. */
export function CanvasSkeleton({ className }: { className?: string }) {
  return (
    <div
      aria-hidden
      className={cn(
        'relative w-full overflow-hidden rounded-lg bg-card ring-1 ring-border',
        '[background-image:radial-gradient(var(--border)_1px,transparent_1px)] [background-size:20px_20px]',
        CANVAS_HEIGHT_CLASS,
        className,
      )}
    >
      <svg className="absolute inset-0 size-full text-border" viewBox="0 0 100 100" preserveAspectRatio="none">
        {LINKS.map((link, index) => (
          <path key={index} d={linkPath(link)} fill="none" stroke="currentColor" strokeWidth={0.3} vectorEffect="non-scaling-stroke" />
        ))}
      </svg>
      {RANKS.map((rows, rank) =>
        rows.map((top) => (
          <Skeleton
            key={`${rank}-${top}`}
            className="absolute rounded-lg"
            style={{ left: `${RANK_LEFT[rank]}%`, top: `${top}%`, width: `${BOX_WIDTH}%`, height: `${BOX_HEIGHT}%`, maxWidth: 176, minHeight: 36 }}
          />
        )),
      )}
    </div>
  )
}

/** Toolbar row plus canvas: the whole workspace while the graph query loads. */
export function GraphSkeleton() {
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-2 lg:flex-row lg:items-center lg:justify-between">
        <div className="flex flex-wrap items-center gap-2">
          <Skeleton className="h-8 w-56" />
          <Skeleton className="h-7 w-24" />
          <Skeleton className="h-7 w-20" />
          <Skeleton className="h-7 w-28" />
          <Skeleton className="h-7 w-24" />
        </div>
        <div className="flex items-center gap-3">
          <Skeleton className="h-4 w-16" />
          <Skeleton className="h-4 w-24" />
          <Skeleton className="h-7 w-20" />
        </div>
      </div>
      <CanvasSkeleton />
    </div>
  )
}
