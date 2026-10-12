import { CheckIcon, CodeIcon, CopyIcon, EyeIcon, ExternalLinkIcon } from 'lucide-react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

import type { RepoFile } from '../../api/queries'
import { formatBytes, languageLabel } from '../../lib/format'
import { baseName } from '../../lib/path'
import { EntryGlyph } from '../entry-glyph'
import { useCopied } from './use-copied'

export type MarkdownMode = 'preview' | 'source'

interface FileHeaderProps {
  file: RepoFile
  lineCount: number
  /** Present for Markdown files only. */
  markdownMode?: MarkdownMode
  onMarkdownModeChange?: (mode: MarkdownMode) => void
}

/**
 * Path, language, size and the file actions. Sticks under the app header and
 * the project band (its height is `--project-band`, set by the project layout).
 */
export function FileHeader({ file, lineCount, markdownMode, onMarkdownModeChange }: FileHeaderProps) {
  const { copied, copy } = useCopied()
  const name = baseName(file.path)

  return (
    <header className="sticky top-[calc(var(--header-h)+var(--project-band,0px))] z-10 flex min-h-11 flex-wrap items-center gap-x-3 gap-y-1.5 rounded-t-lg border-b bg-background/95 px-3 py-2 backdrop-blur-sm">
      <div className="flex min-w-0 flex-1 items-center gap-2">
        <EntryGlyph name={name} isDir={false} language={file.language} className="size-4 shrink-0 text-muted-foreground" />
        <span className="min-w-0 truncate font-mono text-sm" title={file.path}>
          {file.path}
        </span>
        <Badge variant="outline" className="h-4 shrink-0 px-1 text-2xs font-normal text-muted-foreground">
          {languageLabel(file.language)}
        </Badge>
        <span className="hidden shrink-0 text-2xs tabular-nums text-muted-foreground sm:inline">
          {file.binary ? formatBytes(file.size) : `${lineCount.toLocaleString()} ${lineCount === 1 ? 'line' : 'lines'} · ${formatBytes(file.size)}`}
        </span>
      </div>

      <div className="flex shrink-0 items-center gap-1">
        {markdownMode && onMarkdownModeChange ? (
          <ToggleGroup
            type="single"
            size="sm"
            variant="outline"
            spacing={0}
            value={markdownMode}
            onValueChange={(value) => {
              if (value === 'preview' || value === 'source') onMarkdownModeChange(value)
            }}
            aria-label="Markdown view"
            className="mr-1"
          >
            <ToggleGroupItem value="preview" aria-label="Preview">
              <EyeIcon data-icon="inline-start" />
              Preview
            </ToggleGroupItem>
            <ToggleGroupItem value="source" aria-label="Source">
              <CodeIcon data-icon="inline-start" />
              Source
            </ToggleGroupItem>
          </ToggleGroup>
        ) : null}
        <Button variant="ghost" size="xs" onClick={() => copy(file.path)} aria-live="polite">
          {copied ? <CheckIcon data-icon="inline-start" className="text-success" /> : <CopyIcon data-icon="inline-start" />}
          {copied ? 'Copied' : 'Copy path'}
        </Button>
        <Button variant="ghost" size="xs" asChild>
          <a href={file.htmlUrl} target="_blank" rel="noreferrer">
            Raw
            <ExternalLinkIcon data-icon="inline-end" />
          </a>
        </Button>
      </div>
    </header>
  )
}
