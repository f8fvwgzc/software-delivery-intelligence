import { CheckIcon, CopyIcon, DownloadIcon, ReplaceIcon } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { count, formatNumber } from '@/lib/intl'

import { Tag } from '../tag'
import { FileGlyph } from './file-icon'
import { downloadFile, splitLines, type ViewerFile } from './files'

interface CodePaneProps {
  file: ViewerFile
}

const COPIED_FOR = 1_500

/**
 * Read-only source view: mono, line numbers from a CSS counter (so they are
 * never copied with the text), no wrapping, capped height with its own scroll.
 */
export function CodePane({ file }: CodePaneProps) {
  const lines = splitLines(file.content)
  const [copied, setCopied] = useState(false)
  const copiedTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  useEffect(() => () => clearTimeout(copiedTimer.current), [])

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(file.content)
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
        <FileGlyph file={file} className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 truncate font-mono text-xs">{file.path}</span>
        <Tag mono variant="outline">
          {file.language}
        </Tag>
        {file.replacesExisting ? (
          <span className="inline-flex items-center gap-1 text-2xs text-warning">
            <ReplaceIcon className="size-3" aria-hidden="true" />
            replaces existing
          </span>
        ) : null}
        <span className="ml-auto flex items-center gap-1">
          <Button variant="ghost" size="xs" onClick={copy} aria-label={copied ? 'Copied' : `Copy ${file.path}`}>
            {copied ? <CheckIcon data-icon="inline-start" className="text-success" /> : <CopyIcon data-icon="inline-start" />}
            {copied ? 'Copied' : 'Copy'}
          </Button>
          <Button variant="ghost" size="xs" onClick={() => downloadFile(file)} aria-label={`Download ${file.path}`}>
            <DownloadIcon data-icon="inline-start" />
            Download
          </Button>
        </span>
      </header>
      {file.purpose ? <p className="border-b px-3 py-1.5 text-xs text-muted-foreground">{file.purpose}</p> : null}
      <div className="max-h-[32rem] overflow-auto bg-muted/20" style={{ scrollbarGutter: 'stable' }}>
        <pre className="min-w-max py-2 font-mono text-xs leading-5 [counter-reset:line]" tabIndex={0}>
          <code>
            {lines.map((line, index) => (
              <span
                key={index}
                className="block whitespace-pre pr-4 [counter-increment:line] before:sticky before:left-0 before:mr-4 before:inline-block before:w-10 before:bg-muted/20 before:pr-2 before:text-right before:tabular-nums before:text-muted-foreground before:select-none before:content-[counter(line)]"
              >
                {line}
                {'\n'}
              </span>
            ))}
          </code>
        </pre>
      </div>
      <footer className="flex h-7 items-center border-t px-3 text-2xs tabular-nums text-muted-foreground">
        {count(lines.length, 'line', 'lines')} · {formatNumber(file.content.length)} bytes
      </footer>
    </div>
  )
}
