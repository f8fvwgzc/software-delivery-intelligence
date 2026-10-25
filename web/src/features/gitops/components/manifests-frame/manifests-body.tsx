import { useSuspenseQuery } from '@tanstack/react-query'
import { CheckIcon, CopyIcon, FileCodeIcon } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { toast } from 'sonner'

import { PanelEmpty } from '@/components/system/panel'
import { Button } from '@/components/ui/button'
import { count, formatNumber } from '@/lib/intl'

import { gitopsQueries } from '../../api/queries'
import { RevisionLink } from '../revision-link'
import { useYamlTokens } from './use-yaml-tokens'
import { YamlView } from './yaml-view'

const COPIED_FOR = 1_500

/** One YAML stream of every document, `---` between them, however each came back. */
function joinDocuments(documents: readonly string[]): string {
  return documents.map((document) => document.replace(/^---\s*\n/, '').replace(/\s+$/, '')).join('\n---\n')
}

interface ManifestsBodyProps {
  applicationId: string
  /** GitHub URL of the repository it renders, for the revision link. */
  repoUrl?: string
}

/** The rendered manifests: how many documents, from which commit, and the YAML itself. */
export function ManifestsBody({ applicationId, repoUrl }: ManifestsBodyProps) {
  const { data } = useSuspenseQuery(gitopsQueries.manifests(applicationId))
  const text = joinDocuments(data.manifests)
  const lines = text.split('\n')
  const tokens = useYamlTokens(applicationId, data.revision ?? '', text)
  const [copied, setCopied] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  useEffect(() => () => clearTimeout(timer.current), [])

  if (data.manifests.length === 0) {
    return (
      <PanelEmpty
        icon={FileCodeIcon}
        title="Nothing rendered"
        description="The source rendered no documents at this revision. Check the path and the type in the application’s settings."
      />
    )
  }

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      clearTimeout(timer.current)
      timer.current = setTimeout(() => setCopied(false), COPIED_FOR)
    } catch {
      toast.error('Could not copy to the clipboard')
    }
  }

  return (
    <>
      <div className="flex min-h-9 flex-wrap items-center gap-x-3 gap-y-1 border-b px-3 py-1 text-xs text-muted-foreground">
        <span className="tabular-nums">{count(data.manifests.length, 'document', 'documents')}</span>
        <span className="tabular-nums">{count(lines.length, 'line', 'lines')}</span>
        {data.revision ? (
          <span>
            rendered from <RevisionLink revision={data.revision} repoUrl={repoUrl} />
          </span>
        ) : null}
        <span>Secret values are masked.</span>
        <Button variant="ghost" size="xs" className="ml-auto" onClick={copy} aria-label={copied ? 'Copied' : 'Copy the manifests'}>
          {copied ? <CheckIcon data-icon="inline-start" className="text-success" /> : <CopyIcon data-icon="inline-start" />}
          {copied ? 'Copied' : 'Copy'}
        </Button>
      </div>
      <YamlView lines={lines} tokens={tokens} label={`Rendered manifests, ${formatNumber(lines.length)} lines`} />
    </>
  )
}
