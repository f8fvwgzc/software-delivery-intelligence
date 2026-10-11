import { CheckIcon, CopyIcon, DownloadIcon, ReplaceIcon } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'

import { downloadArtifact, splitLines, type Artifact } from '../../lib/artifacts'
import { ArtifactIcon } from './artifact-icon'

interface CodePaneProps {
  artifact: Artifact
}

const COPIED_FOR = 1_500

/**
 * Read-only source view: mono, line numbers from a CSS counter (so they are
 * never copied with the text), no wrapping, capped height with its own scroll.
 */
export function CodePane({ artifact }: CodePaneProps) {
  const lines = splitLines(artifact.content)
  const [copied, setCopied] = useState(false)
  const copiedTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  useEffect(() => () => clearTimeout(copiedTimer.current), [])

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(artifact.content)
      setCopied(true)
      clearTimeout(copiedTimer.current)
      copiedTimer.current = setTimeout(() => setCopied(false), COPIED_FOR)
    } catch {
      toast.error('Could not copy to the clipboard')
    }
  }

  return (
    <div className="flex min-w-0 flex-col">
      <header className="flex min-h-10 flex-wrap items-center gap-x-2 gap-y-1 border-b px-3 py-1.5">
        <ArtifactIcon artifact={artifact} className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 truncate font-mono text-xs">{artifact.path}</span>
        <Badge variant="outline" className="h-4 px-1 font-mono text-2xs font-normal text-muted-foreground">
          {artifact.language}
        </Badge>
        {artifact.replacesExisting ? (
          <span className="inline-flex items-center gap-1 text-2xs text-warning">
            <ReplaceIcon className="size-3" aria-hidden="true" />
            replaces existing
          </span>
        ) : null}
        <span className="ml-auto flex items-center gap-1">
          <Button variant="ghost" size="xs" onClick={copy} aria-label={copied ? 'Copied' : `Copy ${artifact.path}`}>
            {copied ? <CheckIcon data-icon="inline-start" className="text-success" /> : <CopyIcon data-icon="inline-start" />}
            {copied ? 'Copied' : 'Copy'}
          </Button>
          <Button variant="ghost" size="xs" onClick={() => downloadArtifact(artifact)} aria-label={`Download ${artifact.path}`}>
            <DownloadIcon data-icon="inline-start" />
            Download
          </Button>
        </span>
      </header>
      {artifact.purpose ? <p className="border-b px-3 py-1.5 text-xs text-muted-foreground">{artifact.purpose}</p> : null}
      <div className="max-h-[32rem] overflow-auto bg-muted/20" style={{ scrollbarGutter: 'stable' }}>
        <pre className="min-w-max py-2 font-mono text-xs leading-5 [counter-reset:line]" tabIndex={0}>
          <code>
            {lines.map((line, index) => (
              <span
                key={index}
                className="block whitespace-pre pr-4 [counter-increment:line] before:sticky before:left-0 before:mr-4 before:inline-block before:w-10 before:bg-muted/20 before:pr-2 before:text-right before:tabular-nums before:text-muted-foreground/60 before:select-none before:content-[counter(line)]"
              >
                {line}
                {'\n'}
              </span>
            ))}
          </code>
        </pre>
      </div>
      <footer className="flex h-7 items-center border-t px-3 text-2xs tabular-nums text-muted-foreground">
        {lines.length} {lines.length === 1 ? 'line' : 'lines'} · {artifact.content.length.toLocaleString()} bytes
      </footer>
    </div>
  )
}
