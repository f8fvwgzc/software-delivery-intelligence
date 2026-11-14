import { ArrowLeftIcon, ChevronRightIcon, PanelRightOpenIcon, XIcon } from 'lucide-react'
import { Fragment } from 'react'

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

import type { Board } from '../../lib/board'

interface BoardHeaderProps {
  board: Board
  /** Go to a crumb of the trail, by index. */
  onCrumb: (index: number) => void
  onUp: () => void
  onClearSearch: () => void
  /** Phones: open the details sheet (the board summary, or the selected node). */
  onShowDetails?: () => void
}

/** Height the header takes over the canvas; the canvas keeps cards out from under it when fitting. */
export const BOARD_HEADER_INSET = 112

/**
 * Where you are on the map, laid over the top of the canvas: the trail of
 * containers (Overview / part / ...), the up control, and the current board's
 * title and description, after Floorplan's context header. Only the controls
 * take the pointer; the rest lets pans through.
 */
export function BoardHeader({ board, onCrumb, onUp, onClearSearch, onShowDetails }: BoardHeaderProps) {
  const searching = board.scope.type === 'search'
  const nested = board.trail.length > 1

  return (
    <div className="pointer-events-none absolute inset-x-0 top-0 z-10 flex flex-col gap-1.5 p-3">
      <nav aria-label="Map level" className="flex min-w-0 items-center gap-1">
        {nested ? (
          <Button
            variant="ghost"
            size="icon-xs"
            className="pointer-events-auto"
            aria-label="Up one level"
            title="Up one level (Backspace)"
            onClick={onUp}
          >
            <ArrowLeftIcon />
          </Button>
        ) : null}
        <ol className="flex min-w-0 items-center gap-0.5 text-xs">
          {board.trail.map((crumb, index) => {
            const current = !searching && index === board.trail.length - 1
            return (
              <Fragment key={crumb.nodeId ?? 'overview'}>
                {index > 0 ? <ChevronRightIcon aria-hidden className="size-3 shrink-0 text-muted-foreground/60" /> : null}
                <li className="min-w-0">
                  <button
                    type="button"
                    aria-current={current ? 'location' : undefined}
                    className={cn(
                      'pointer-events-auto max-w-48 truncate rounded-sm px-1.5 py-0.5 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring',
                      current ? 'font-medium text-foreground' : 'text-muted-foreground',
                    )}
                    onClick={() => onCrumb(index)}
                  >
                    {crumb.label}
                  </button>
                </li>
              </Fragment>
            )
          })}
        </ol>
        {searching ? (
          <Button variant="ghost" size="xs" className="pointer-events-auto ml-1 text-muted-foreground" onClick={onClearSearch}>
            <XIcon data-icon="inline-start" />
            Clear search
          </Button>
        ) : null}
        {onShowDetails ? (
          <Button variant="outline" size="xs" className="pointer-events-auto ml-auto" onClick={onShowDetails}>
            <PanelRightOpenIcon data-icon="inline-start" />
            Details
          </Button>
        ) : null}
      </nav>
      {/* Announced when the board changes, so moving through the map is heard as well as seen. */}
      <div className="flex max-w-md flex-col gap-0.5 pl-1.5" aria-live="polite" aria-atomic="true">
        {board.caption ? <span className="text-2xs leading-4 text-muted-foreground">{board.caption}</span> : null}
        <h2 className="truncate text-[15px] leading-5 font-semibold text-foreground">{board.title}</h2>
        {board.description ? <p className="line-clamp-2 text-xs text-muted-foreground">{board.description}</p> : null}
      </div>
    </div>
  )
}
