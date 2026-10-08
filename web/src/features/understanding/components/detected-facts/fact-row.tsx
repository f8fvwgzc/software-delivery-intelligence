import { ExternalLinkIcon } from 'lucide-react'

import type { Project } from '@/features/projects/api/queries'

import type { Fact } from '../../api/queries'
import { sourceFileUrl } from '../../lib/github'

interface FactRowProps {
  fact: Fact
  project: Pick<Project, 'repoUrl' | 'defaultBranch'>
}

/** One `dt`/`dd` pair: what was found, the excerpt that proves it, and where. */
export function FactRow({ fact, project }: FactRowProps) {
  const href = sourceFileUrl(project, fact.sourcePath)

  return (
    <>
      <dt className="border-b border-border/60 py-2 pr-2 text-sm">{fact.label}</dt>
      <dd className="flex min-w-0 flex-col gap-0.5 border-b border-border/60 py-2">
        <span className="truncate text-sm" title={fact.value}>
          {fact.value || <span className="text-muted-foreground">present</span>}
        </span>
        {fact.evidence ? (
          <code className="truncate text-2xs text-muted-foreground" title={fact.evidence}>
            {fact.evidence}
          </code>
        ) : null}
        {href ? (
          <a
            href={href}
            target="_blank"
            rel="noreferrer"
            className="inline-flex w-fit max-w-full items-center gap-1 rounded-sm font-mono text-2xs text-muted-foreground outline-none hover:text-foreground hover:underline hover:underline-offset-2 focus-visible:ring-2 focus-visible:ring-ring/50"
          >
            <span className="truncate">{fact.sourcePath}</span>
            <ExternalLinkIcon className="size-2.5 shrink-0" aria-hidden />
          </a>
        ) : null}
      </dd>
    </>
  )
}
