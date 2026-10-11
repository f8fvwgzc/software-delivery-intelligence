import mermaid from 'mermaid'
import { useTheme } from 'next-themes'
import { useEffect, useId, useRef, useState } from 'react'

import { Spinner } from '@/components/ui/spinner'
import { cn } from '@/lib/utils'

import { readTokens, themeVariables } from './theme'

interface MermaidPreviewProps {
  /** Already debounced by the editor. */
  source: string
  /** Reports the parse error of the latest source, or null once it renders. */
  onError: (message: string | null) => void
  className?: string
}

interface Rendered {
  /** The source and theme this drawing was made from. */
  key: string
  svg: string
}

function messageOf(error: unknown): string {
  if (error instanceof Error) return error.message
  return typeof error === 'string' ? error : 'The diagram could not be rendered.'
}

/**
 * Renders Mermaid source with the `base` theme mapped from the design
 * tokens, re-rendering when the source or the light/dark theme changes. A
 * failed render keeps the last good drawing on screen while the error is
 * reported to the editor.
 */
export function MermaidPreview({ source, onError, className }: MermaidPreviewProps) {
  const { resolvedTheme } = useTheme()
  const id = useId().replace(/[^a-zA-Z0-9]/g, '')
  const sequence = useRef(0)
  const [rendered, setRendered] = useState<Rendered | null>(null)
  const [settled, setSettled] = useState('')
  const onErrorRef = useRef(onError)
  const key = `${resolvedTheme}\u0000${source}`
  const rendering = settled !== key

  useEffect(() => {
    onErrorRef.current = onError
  }, [onError])

  useEffect(() => {
    let cancelled = false

    const run = async () => {
      try {
        const tokens = readTokens()
        mermaid.initialize({
          startOnLoad: false,
          theme: 'base',
          securityLevel: 'strict',
          fontFamily: tokens.fontSans,
          themeVariables: themeVariables(tokens, resolvedTheme === 'dark'),
          flowchart: { htmlLabels: true, curve: 'basis', padding: 12, nodeSpacing: 32, rankSpacing: 56 },
          sequence: { useMaxWidth: true },
        })
        await mermaid.parse(source)
        const result = await mermaid.render(`mmd${id}x${sequence.current++}`, source)
        if (cancelled) return
        setRendered({ key, svg: result.svg })
        onErrorRef.current(null)
      } catch (error) {
        if (!cancelled) onErrorRef.current(messageOf(error))
      } finally {
        if (!cancelled) setSettled(key)
      }
    }
    void run()

    return () => {
      cancelled = true
    }
  }, [source, resolvedTheme, id, key])

  return (
    <div className={cn('relative overflow-auto', className)} aria-busy={rendering || undefined}>
      {rendering ? <Spinner className="absolute top-3 right-3 size-3.5 text-muted-foreground" /> : null}
      {rendered ? (
        // Mermaid output is sanitised by DOMPurify under `securityLevel: strict`.
        <div
          className={cn('p-4 transition-opacity [&_svg]:mx-auto [&_svg]:h-auto [&_svg]:max-w-full', rendering && 'opacity-70')}
          dangerouslySetInnerHTML={{ __html: rendered.svg }}
        />
      ) : null}
    </div>
  )
}
