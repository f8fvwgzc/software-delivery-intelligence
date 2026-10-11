import { useSuspenseQuery } from '@tanstack/react-query'
import { FileXIcon, TriangleAlertIcon } from 'lucide-react'
import { useState } from 'react'
import { useLocation } from 'react-router'

import { Markdown } from '@/components/shared/data/markdown'
import { EmptyState } from '@/components/shared/feedback'

import { codeQueries } from '../../api/queries'
import { formatBytes, isMarkdown, parseLineHash, splitLines } from '../../lib/format'
import { baseName } from '../../lib/path'
import { CodeLines } from './code-lines'
import { FactsBar } from './facts-bar'
import { FileHeader, type MarkdownMode } from './file-header'

interface FileViewProps {
  projectId: string
  path: string
}

/**
 * One file at the scanned commit. Suspends on its own query; the caller
 * wraps it in <FileBoundary> and keys it by path so view state resets.
 */
export function FileView({ projectId, path }: FileViewProps) {
  const { data: file } = useSuspenseQuery(codeQueries.file(projectId, path))
  const { hash } = useLocation()
  const [markdownMode, setMarkdownMode] = useState<MarkdownMode>('preview')

  const markdown = !file.binary && isMarkdown(file.language, baseName(file.path))
  const lines = file.binary ? [] : splitLines(file.content)
  const highlighted = parseLineHash(hash)

  return (
    <article className="flex min-w-0 flex-col" aria-label={file.path}>
      <FileHeader
        file={file}
        lineCount={lines.length}
        markdownMode={markdown ? markdownMode : undefined}
        onMarkdownModeChange={markdown ? setMarkdownMode : undefined}
      />
      {file.factIds.length > 0 ? <FactsBar projectId={projectId} factIds={file.factIds} /> : null}
      {file.truncated ? (
        <p className="flex items-center gap-1.5 border-b bg-warning/5 px-3 py-1.5 text-xs text-warning">
          <TriangleAlertIcon className="size-3.5 shrink-0" aria-hidden="true" />
          Showing the first 1 MiB of {formatBytes(file.size)}. Open on GitHub for the whole file.
        </p>
      ) : null}
      {file.binary ? (
        <EmptyState
          icon={FileXIcon}
          title="Binary file"
          description={`${formatBytes(file.size)} that is not text. View it on GitHub instead.`}
          className="m-4"
        />
      ) : markdown && markdownMode === 'preview' ? (
        <Markdown className="max-w-[80ch] px-5 py-4">{file.content}</Markdown>
      ) : (
        <CodeLines lines={lines} highlighted={highlighted} />
      )}
    </article>
  )
}

export { FileBoundary } from './file-boundary'
export { FileViewSkeleton } from './file-skeleton'
