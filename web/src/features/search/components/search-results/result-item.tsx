import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { ScoreBar } from '@/components/shared/data'
import type { components } from '@/lib/api/schema'

import { KNOWLEDGE_KIND } from '../../lib/labels'
import { HighlightedText } from './highlighted-text'

type KnowledgeHit = components['schemas']['KnowledgeHit']

/** Where a hit leads, when it leads somewhere in the app. */
function targetFor(hit: KnowledgeHit) {
  switch (hit.kind) {
    case 'decision':
      return routes.decisions()
    case 'pattern':
      return routes.patterns()
    case 'conversation':
      return hit.refId ? routes.proposal(hit.refId) : undefined
    case 'document':
      return hit.projectId ? routes.projectDocuments(hit.projectId) : undefined
    case 'architecture':
      return hit.projectId ? routes.projectArchitecture(hit.projectId) : undefined
    default:
      return hit.projectId ? routes.project(hit.projectId) : undefined
  }
}

export function ResultItem({ hit, query }: { hit: KnowledgeHit; query: string }) {
  const kind = KNOWLEDGE_KIND[hit.kind]
  const to = targetFor(hit)

  return (
    <li className="flex gap-3 px-4 py-3">
      <kind.icon className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-label={kind.label} />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
          {to ? (
            <Link to={to} className="truncate text-sm font-medium hover:underline">
              <HighlightedText text={hit.title || hit.path || kind.label} query={query} />
            </Link>
          ) : (
            <span className="truncate text-sm font-medium">
              <HighlightedText text={hit.title || hit.path || kind.label} query={query} />
            </span>
          )}
          <span className="text-xs text-muted-foreground">{hit.projectName}</span>
        </div>
        {hit.path ? <span className="truncate font-mono text-2xs text-muted-foreground">{hit.path}</span> : null}
        {hit.snippet ? (
          <p className="line-clamp-3 text-xs leading-5 text-muted-foreground">
            <HighlightedText text={hit.snippet} query={query} />
          </p>
        ) : null}
      </div>
      <div className="hidden w-28 shrink-0 flex-col items-end gap-1 sm:flex">
        <span className="text-2xs text-muted-foreground">{kind.label}</span>
        <ScoreBar value={hit.score} className="w-full" />
      </div>
    </li>
  )
}
