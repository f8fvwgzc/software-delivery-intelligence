import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { useDeferredValue, useState } from 'react'

import { ErrorState } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'
import { Frame } from '@/components/system/frame'

import { searchQuery } from '../api/queries'
import { SearchForm } from '../components/search-form'
import { SearchIdle, SearchResults } from '../components/search-results'
import { SearchResultsSkeleton } from '../components/search-results/skeleton'
import { MIN_QUERY_LENGTH } from '../lib/labels'
import { count } from '@/lib/intl'

/** /knowledge/search — semantic search over everything indexed. */
export default function SearchPage() {
  const [query, setQuery] = useState('')
  const [projectId, setProjectId] = useState('')
  const [kinds, setKinds] = useState<string[]>([])

  // The input stays responsive; the request follows the deferred value.
  const deferredQuery = useDeferredValue(query.trim())
  const active = deferredQuery.length >= MIN_QUERY_LENGTH
  const filters = { q: deferredQuery, projectId: projectId || undefined, kinds: kinds.toSorted() }

  const results = useQuery({
    ...searchQuery(filters),
    enabled: active,
    placeholderData: keepPreviousData,
  })

  const searching = active && (results.isFetching || deferredQuery !== query.trim())
  const filtered = projectId !== '' || kinds.length > 0

  return (
    <Page>
      <PageHeader title="Search" description="Ranked by meaning, not by keyword. Two characters start a search." />
      <SearchForm
        query={query}
        onQueryChange={setQuery}
        projectId={projectId}
        onProjectChange={setProjectId}
        kinds={kinds}
        onKindsChange={setKinds}
        searching={searching}
      />
      <Frame
        title="Results"
        padding="none"
        meta={
          active && results.data ? (
            <span className="text-2xs tabular-nums text-muted-foreground" aria-live="polite">
              {count(results.data.length, 'hit', 'hits')}
            </span>
          ) : undefined
        }
      >
        {!active ? (
          <SearchIdle />
        ) : results.isPending ? (
          <SearchResultsSkeleton />
        ) : results.isError ? (
          <ErrorState compact error={results.error} onRetry={() => results.refetch()} retrying={results.isFetching} />
        ) : (
          <SearchResults
            hits={results.data}
            query={deferredQuery}
            stale={results.isPlaceholderData || results.isFetching}
            filtered={filtered}
            onClearFilters={() => {
              setProjectId('')
              setKinds([])
            }}
          />
        )}
      </Frame>
    </Page>
  )
}
