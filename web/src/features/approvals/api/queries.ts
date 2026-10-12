import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import { fallbackPoll } from '@/lib/live/connection-store'

import { approvalKeys, type ApprovalListFilters } from './keys'

export const approvalQueries = {
  list: (filters: ApprovalListFilters = {}) =>
    queryOptions({
      queryKey: approvalKeys.list(filters),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/approvals', { params: { query: filters }, signal })),
    }),

  /** The human work queue. */
  pending: () =>
    queryOptions({
      ...approvalQueries.list({ status: 'pending' }),
      refetchInterval: () => fallbackPoll(5_000),
    }),

  /** Everything already decided or expired, newest first. */
  history: () =>
    queryOptions({
      ...approvalQueries.list({}),
      refetchInterval: () => fallbackPoll(5_000),
      select: (approvals) =>
        approvals
          .filter((approval) => approval.status !== 'pending')
          .sort((a, b) => (b.decidedAt ?? b.requestedAt).localeCompare(a.decidedAt ?? a.requestedAt)),
    }),
}
