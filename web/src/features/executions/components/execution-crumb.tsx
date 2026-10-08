import { useSuspenseQuery } from '@tanstack/react-query'
import { useParams } from 'react-router'

import { executionQueries } from '../api/queries'

/** Breadcrumb for `/executions/:executionId`: "#12 Deploy to staging". */
export default function ExecutionCrumb() {
  const { executionId = '' } = useParams()
  const { data: execution } = useSuspenseQuery(executionQueries.detail(executionId))

  return (
    <span className="inline-flex max-w-64 items-baseline gap-1.5">
      <span className="font-mono tabular-nums text-muted-foreground">#{execution.number}</span>
      <span className="truncate">{execution.title}</span>
    </span>
  )
}
