import { cn } from '@/lib/utils'

interface CodeLinesProps {
  lines: string[]
  /** 1-based line from the `#L42` hash. */
  highlighted: number | null
}

/** Brings the anchored line into view when it mounts. */
function revealLine(element: HTMLDivElement | null) {
  element?.scrollIntoView({ block: 'center' })
}

const LINE_CLASS =
  'group/line flex min-w-max [counter-increment:line] hover:bg-muted/40 data-highlighted:bg-brand/8'

const GUTTER_CLASS =
  'sticky left-0 z-[1] w-14 shrink-0 select-none bg-background pr-3 text-right tabular-nums text-muted-foreground/55 outline-none group-hover/line:text-muted-foreground group-data-highlighted/line:bg-brand/8 group-data-highlighted/line:text-foreground hover:text-foreground focus-visible:text-foreground before:content-[counter(line)]'

/**
 * Read-only source: 12.5px mono, no wrapping, line numbers from a CSS
 * counter (never part of a copy) that also serve as `#L<n>` anchors.
 */
export function CodeLines({ lines, highlighted }: CodeLinesProps) {
  return (
    <div className="overflow-x-auto" style={{ scrollbarGutter: 'stable' }}>
      <pre className="min-w-max py-2 font-mono text-[12.5px] leading-5 [counter-reset:line]" tabIndex={0}>
        {lines.map((line, index) => {
          const number = index + 1
          const isHighlighted = number === highlighted
          return (
            <div
              key={number}
              id={`L${number}`}
              data-highlighted={isHighlighted || undefined}
              ref={isHighlighted ? revealLine : undefined}
              className={LINE_CLASS}
            >
              <a href={`#L${number}`} aria-label={`Line ${number}`} className={GUTTER_CLASS} />
              <code className={cn('whitespace-pre pr-6', line === '' && 'before:content-["\\00a0"]')}>{line}</code>
            </div>
          )
        })}
      </pre>
    </div>
  )
}
