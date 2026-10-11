import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'

/** Height shared by the canvas, the Mermaid preview and their skeletons, so switching tabs never jumps. */
export const CANVAS_HEIGHT_CLASS = 'h-[max(420px,calc(100dvh-19rem))]'

interface GroupSpec {
  left: number
  top: number
  width: number
  height: number
  cards: readonly (readonly [left: number, top: number])[]
}

/** Two tinted regions with cards in them, joined by faint elbow links, like a laid-out spec (percent units). */
const GROUPS: readonly GroupSpec[] = [
  { left: 5, top: 12, width: 26, height: 62, cards: [[8, 22], [8, 46]] },
  { left: 40, top: 8, width: 26, height: 40, cards: [[43, 18], [43, 34]] },
]
const LOOSE_CARDS: readonly (readonly [left: number, top: number])[] = [
  [74, 26],
  [74, 56],
]
const CARD_W = 20
const CARD_H = 9

const LINKS = [
  'M 28 26.5 H 36 Q 38 26.5 38 24.5 V 22.5 Q 38 22.5 40 22.5 H 43',
  'M 28 50.5 H 36 Q 38 50.5 38 48.5 V 38.5 H 43',
  'M 63 22.5 H 70 Q 72 22.5 72 24.5 V 30.5 H 74',
  'M 63 38.5 H 70 Q 72 38.5 72 40.5 V 60.5 H 74',
]

/** The canvas alone, while the renderer chunk loads. */
export function CanvasSkeleton({ className }: { className?: string }) {
  return (
    <div
      aria-hidden
      className={cn('relative w-full overflow-hidden rounded-lg bg-card ring-1 ring-border', CANVAS_HEIGHT_CLASS, className)}
    >
      {GROUPS.map((group) => (
        <div
          key={group.left}
          className="absolute rounded-lg bg-muted/40 ring-1 ring-border"
          style={{ left: `${group.left}%`, top: `${group.top}%`, width: `${group.width}%`, height: `${group.height}%` }}
        >
          <Skeleton className="absolute top-2 left-3 h-2 w-12" />
        </div>
      ))}
      <svg className="absolute inset-0 size-full text-border" viewBox="0 0 100 100" preserveAspectRatio="none">
        {LINKS.map((d) => (
          <path key={d} d={d} fill="none" stroke="currentColor" strokeWidth={0.4} vectorEffect="non-scaling-stroke" />
        ))}
      </svg>
      {[...GROUPS.flatMap((group) => group.cards), ...LOOSE_CARDS].map(([left, top]) => (
        <div
          key={`${left}-${top}`}
          className="absolute flex items-center gap-2 rounded-md bg-card px-2 ring-1 ring-border"
          style={{ left: `${left}%`, top: `${top}%`, width: `${CARD_W}%`, height: `${CARD_H}%`, maxWidth: 200, minHeight: 44 }}
        >
          <Skeleton className="size-4 shrink-0 rounded-sm" />
          <div className="flex min-w-0 flex-1 flex-col gap-1.5">
            <Skeleton className="h-2.5 w-3/4" />
            <Skeleton className="h-2 w-1/2" />
          </div>
        </div>
      ))}
      <div className="absolute right-3 bottom-3 flex items-center gap-1 rounded-lg p-1 ring-1 ring-border">
        <Skeleton className="size-7" />
        <Skeleton className="h-3 w-11" />
        <Skeleton className="size-7" />
        <Skeleton className="size-7" />
      </div>
    </div>
  )
}
