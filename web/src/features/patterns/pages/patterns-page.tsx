import { useSuspenseQuery } from '@tanstack/react-query'
import { useState, useTransition } from 'react'

import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'

import { patternsQuery } from '../api/queries'
import { PatternFilters } from '../components/pattern-filters'
import { PatternTable } from '../components/pattern-table'
import { PatternTableSkeleton } from '../components/pattern-table/skeleton'

/** /knowledge/patterns — the catalog the ranker draws from, plus what it learned. */
export default function PatternsPage() {
  const [category, setCategory] = useState('')
  const [includeDeprecated, setIncludeDeprecated] = useState(false)
  const [isPending, startTransition] = useTransition()

  return (
    <Page wide>
      <PageHeader
        title="Patterns"
        description="Reusable, outcome-tracked ways of doing something. Catalog patterns ship with the platform; learned ones come from your deployments."
      />
      <PatternFilters
        category={category}
        onCategoryChange={(next) => startTransition(() => setCategory(next))}
        includeDeprecated={includeDeprecated}
        onIncludeDeprecatedChange={(next) => startTransition(() => setIncludeDeprecated(next))}
        busy={isPending}
      />
      <QueryBoundary fallback={<PatternTableSkeleton />}>
        <PatternsBody category={category} includeDeprecated={includeDeprecated} />
      </QueryBoundary>
    </Page>
  )
}

function PatternsBody({ category, includeDeprecated }: { category: string; includeDeprecated: boolean }) {
  const { data } = useSuspenseQuery(patternsQuery({ category: category || undefined, includeDeprecated }))
  return <PatternTable patterns={data} filtered={category !== '' || includeDeprecated} />
}
