import { useEffect, useRef, type CSSProperties } from 'react'

import { useWindowRows } from '@/components/system/virtual-rows'
import { cn } from '@/lib/utils'

import type { TokenLines } from './use-highlight'

interface CodeLinesProps {
  lines: string[]
  /** Syntax tokens per line; absent while the file renders as plain text. */
  tokens?: TokenLines
  /** 1-based line from the `#L42` hash. */
  highlighted: number | null
}

/** Every line is exactly this tall (`leading-5`): the windowing math reads it. */
export const LINE_HEIGHT = 20
/** Files longer than this keep only the lines on screen in the DOM. */
const VIRTUAL_LINES = 1_500

const LINE_CLASS =
  'group/line flex min-w-max hover:bg-muted/40 data-highlighted:bg-brand/8'

const GUTTER_CLASS =
  'sticky left-0 z-[1] w-14 shrink-0 select-none bg-background pr-3 text-right tabular-nums text-muted-foreground outline-none group-hover/line:text-muted-foreground group-data-highlighted/line:bg-brand/8 group-data-highlighted/line:text-foreground hover:text-foreground focus-visible:text-foreground before:content-[attr(data-line)]'

/**
 * Read-only source: 12.5px mono, no wrapping, line numbers from a data
 * attribute (never part of a copy) that also serve as `#L<n>` anchors.
 * With tokens, each span carries both themes' colours as CSS variables and
 * `.code-token` (index.css) resolves them, so a theme switch needs no re-render.
 * A long file (a megabyte, tens of thousands of lines) is windowed against
 * the page scroll: only the visible band plus an overscan is rendered.
 */
export function CodeLines({ lines, tokens, highlighted }: CodeLinesProps) {
  const preRef = useRef<HTMLPreElement>(null)
  const rows = useWindowRows({ count: lines.length, estimateSize: LINE_HEIGHT, listRef: preRef, threshold: VIRTUAL_LINES, overscan: 30 })
  const { scrollToIndex } = rows

  // Bring the anchored line into view once, when the file (or the hash) lands.
  useEffect(() => {
    if (highlighted === null || highlighted > lines.length) return
    scrollToIndex(highlighted - 1, { align: 'center' })
  }, [highlighted, lines.length, scrollToIndex])

  return (
    <div className="overflow-x-auto" style={{ scrollbarGutter: 'stable' }}>
      <pre
        ref={preRef}
        className={cn('min-w-max py-2 font-mono text-[12.5px] leading-5', rows.virtual && 'relative')}
        style={rows.virtual ? { height: rows.totalSize + 16 } : undefined}
        tabIndex={0}
        data-virtual={rows.virtual || undefined}
      >
        {rows.items.map((item) => {
          const index = item.index
          const line = lines[index]
          if (line === undefined) return null
          const number = index + 1
          const isHighlighted = number === highlighted
          const lineTokens = tokens?.[index]
          const style: CSSProperties | undefined = rows.virtual
            ? { position: 'absolute', top: 8, left: 0, minWidth: '100%', height: LINE_HEIGHT, transform: `translateY(${item.start - (rows.items[0]?.start ?? 0) + rows.offsetTop}px)` }
            : undefined
          return (
            <div key={number} id={`L${number}`} data-highlighted={isHighlighted || undefined} className={LINE_CLASS} style={style}>
              <a href={`#L${number}`} aria-label={`Line ${number}`} data-line={number} className={GUTTER_CLASS} />
              <code className={cn('whitespace-pre pr-6', line === '' && 'before:content-["\\00a0"]')}>
                {lineTokens
                  ? lineTokens.map((token, offset) => (
                      <span key={offset} className="code-token" style={token.htmlStyle as CSSProperties}>
                        {token.content}
                      </span>
                    ))
                  : line}
              </code>
            </div>
          )
        })}
      </pre>
    </div>
  )
}
