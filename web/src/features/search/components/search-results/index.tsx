import { SearchIcon, SearchXIcon } from 'lucide-react'

import { EmptyState } from '@/components/shared/feedback'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { ResultItem } from './result-item'
import { count } from '@/lib/intl'

type KnowledgeHit = components['schemas']['KnowledgeHit']

interface SearchResultsProps {
  hits: KnowledgeHit[]
  query: string
  /** True when results are for an older query and a newer one is in flight. */
  stale: boolean
  /** True when a project or kind filter narrows the search. */
  filtered: boolean
  onClearFilters: () => void
}

export function SearchResults({ hits, query, stale, filtered, onClearFilters }: SearchResultsProps) {
  if (hits.length === 0) {
    return (
      <EmptyState
        icon={SearchXIcon}
        title={`No matches for “${query}”`}
        description={
          filtered
            ? 'Nothing indexed matches within the current project or kinds.'
            : 'Try different words. Search is semantic, so a description of what you mean works too.'
        }
        action={
          filtered ? (
            <button
              type="button"
              className="text-sm text-muted-foreground underline-offset-4 hover:underline"
              onClick={onClearFilters}
            >
              Search everything instead
            </button>
          ) : undefined
        }
      />
    )
  }

  return (
    <div className={cn('flex flex-col gap-2 transition-opacity duration-150', stale && 'opacity-60')} aria-busy={stale || undefined}>
      <p className="text-xs tabular-nums text-muted-foreground">
        {count(hits.length, 'match', 'matches')}, best first
      </p>
      <ol className="flex flex-col divide-y divide-border/60 rounded-lg bg-card ring-1 ring-border">
        {hits.map((hit) => (
          <ResultItem key={hit.id} hit={hit} query={query} />
        ))}
      </ol>
    </div>
  )
}

/** Before any query is typed. */
export function SearchIdle() {
  return (
    <EmptyState
      icon={SearchIcon}
      title="Search what the platform knows"
      description="Code, infrastructure, pipelines, architecture, generated documents, patterns, decisions and past negotiations, ranked by meaning."
    />
  )
}
