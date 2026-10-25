import { useRef, type CSSProperties } from 'react'
import type { ThemedToken } from 'shiki/core'

import { useScrollRows } from '@/components/system/virtual-rows'
import { cn } from '@/lib/utils'

/** Every line is exactly this tall (`leading-5`): the windowing math reads it. */
const LINE_HEIGHT = 20
/** Longer manifests keep only the lines in view in the DOM. */
const VIRTUAL_LINES = 1_500
const PAD = 8

interface YamlViewProps {
  lines: readonly string[]
  tokens?: ThemedToken[][]
  label: string
}

const GUTTER =
  'sticky left-0 z-[1] w-12 shrink-0 select-none bg-background pr-3 text-right tabular-nums text-muted-foreground before:content-[attr(data-line)]'

/**
 * Read-only YAML in its own scroll box: mono, no wrapping, line numbers
 * from a data attribute (never part of a copy), `---` between documents
 * muted. Tokens carry both themes' colours as CSS variables (`.code-token`
 * in index.css), so a theme switch needs no re-render. Past 1 500 lines
 * only the window in view is rendered.
 */
export function YamlView({ lines, tokens, label }: YamlViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const rows = useScrollRows({ count: lines.length, estimateSize: LINE_HEIGHT, scrollRef, threshold: VIRTUAL_LINES, overscan: 30 })

  return (
    <div ref={scrollRef} className="max-h-[36rem] overflow-auto" style={{ scrollbarGutter: 'stable' }}>
      <pre
        className={cn('min-w-max font-mono text-xs leading-5', rows.virtual ? 'relative' : 'py-2')}
        style={rows.virtual ? { height: rows.totalSize + PAD * 2 } : undefined}
        tabIndex={0}
        aria-label={label}
        data-virtual={rows.virtual || undefined}
      >
        {rows.items.map((item) => {
          const line = lines[item.index]
          if (line === undefined) return null
          const lineTokens = tokens?.[item.index]
          const separator = line === '---'
          const style: CSSProperties | undefined = rows.virtual
            ? { position: 'absolute', top: PAD, left: 0, minWidth: '100%', height: LINE_HEIGHT, transform: `translateY(${item.start}px)` }
            : undefined
          return (
            <div key={item.index} className={cn('flex', separator && 'text-muted-foreground')} style={style}>
              <span className={GUTTER} data-line={item.index + 1} aria-hidden />
              <code className={cn('whitespace-pre pr-6', line === '' && 'before:content-["\\00a0"]')}>
                {lineTokens && !separator
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
