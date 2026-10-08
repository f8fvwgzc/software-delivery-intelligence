import { CheckIcon, CopyIcon, DownloadIcon } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { RelativeTime } from '@/components/shared/data'
import { Button } from '@/components/ui/button'
import type { components } from '@/lib/api/schema'

import { DOCUMENT_KIND_LABEL, documentFileName } from '../../lib/labels'

type Document = components['schemas']['Document']

export function ViewerToolbar({ document }: { document: Document }) {
  const [copied, setCopied] = useState(false)

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(document.markdown)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1500)
    } catch {
      toast.error('Could not copy', { description: 'Your browser blocked clipboard access.' })
    }
  }

  const download = () => {
    const blob = new Blob([document.markdown], { type: 'text/markdown;charset=utf-8' })
    const url = URL.createObjectURL(blob)
    const anchor = window.document.createElement('a')
    anchor.href = url
    anchor.download = documentFileName(document.title, document.kind)
    anchor.click()
    URL.revokeObjectURL(url)
  }

  return (
    <header className="flex flex-col gap-2 px-4 py-3 sm:flex-row sm:items-start sm:justify-between md:px-6">
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="text-xs text-muted-foreground">{DOCUMENT_KIND_LABEL[document.kind]}</span>
        <h2 className="truncate text-base font-semibold tracking-tight">{document.title}</h2>
        <p className="flex flex-wrap items-center gap-x-2 text-2xs text-muted-foreground">
          <span>
            Generated <RelativeTime value={document.generatedAt} className="font-mono" />
          </span>
          <span aria-hidden="true">·</span>
          <span className="tabular-nums">{document.factsUsed} facts used</span>
          {document.commitSha ? (
            <>
              <span aria-hidden="true">·</span>
              <span className="font-mono" title={document.commitSha}>
                {document.commitSha.slice(0, 7)}
              </span>
            </>
          ) : null}
        </p>
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <Button size="sm" variant="outline" onClick={copy} aria-live="polite">
          {copied ? <CheckIcon data-icon="inline-start" className="text-success" /> : <CopyIcon data-icon="inline-start" />}
          {copied ? 'Copied' : 'Copy'}
        </Button>
        <Button size="sm" variant="outline" onClick={download}>
          <DownloadIcon data-icon="inline-start" />
          Download .md
        </Button>
      </div>
    </header>
  )
}
