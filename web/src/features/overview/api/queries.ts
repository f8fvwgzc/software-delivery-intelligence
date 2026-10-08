import { queryOptions } from '@tanstack/react-query'

import { approvalQueries } from '@/features/approvals/api/queries'
import { executionQueries } from '@/features/executions/api/queries'
import { projectQueries } from '@/features/projects/api/queries'
import { proposalsQuery } from '@/features/proposals/api/queries'

import { selectRecentExecutions, selectRecentProposals } from '../lib/recent'

/** Newest executions the overview looks at for paused and active ones. */
const EXECUTION_WINDOW = 100

/*
 * The overview owns no entity. Every list comes from the feature that owns
 * it, so cache entries, polling and invalidation are shared with that
 * feature's pages. The `recent*` variants reuse the same cache entry with a
 * `select`, so they cost no extra request.
 */
export const overviewQueries = {
  projects: () => projectQueries.list(),

  pendingApprovals: () => approvalQueries.pending(),

  executions: () => executionQueries.list({ limit: EXECUTION_WINDOW }),

  recentExecutions: () =>
    queryOptions({
      ...executionQueries.list({ limit: EXECUTION_WINDOW }),
      select: selectRecentExecutions,
    }),

  proposals: () => proposalsQuery(),

  recentProposals: () =>
    queryOptions({
      ...proposalsQuery(),
      select: selectRecentProposals,
    }),
}
